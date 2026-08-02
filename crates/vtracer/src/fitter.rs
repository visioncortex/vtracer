//! Curve fitters: turn a region's pixel mask into vector outlines.
//!
//! The three built-ins wrap the corresponding visioncortex tracing modes and
//! emit [`FittedGeom`] contours in absolute (document) coordinates:
//!
//! * [`PixelFitter`] — exact lattice polyline (no simplification).
//! * [`PolygonFitter`] — staircase-symmetric Douglas–Peucker polygon.
//! * [`SplineFitter`] — subdivision + corner detection + least-squares cubics.
//!
//! All three trace *closed* region outlines (outer ring plus holes). The
//! mosaic compositor fits open boundary segments instead; see
//! [`crate::mosaic::SegmentFitter`].

use visioncortex::clusters::Cluster as BinaryCluster;
use visioncortex::{
    CompoundPath, CompoundPathElement, PathSimplifyMode, PointF64, PointI32,
};

use crate::ir::{PathCmd, RegionMask, SubPath};

/// Fitted geometry for one contour — the common currency between the curve
/// fitters, the [`CurvePass`](crate::simplify::CurvePass) stage, and
/// composition. The stacked fitters produce one per closed outline; the
/// mosaic fitters produce one per shared boundary segment.
#[derive(Clone, Debug)]
pub enum FittedGeom {
    /// Polyline (pixel / polygon backends).
    Polyline(Vec<PointF64>),
    /// Chain of cubic Béziers; consecutive curves share endpoints (spline backend).
    Beziers(Vec<[PointF64; 4]>),
}

impl FittedGeom {
    /// Convert one closed contour into a `MoveTo … Close` subpath.
    pub fn into_closed_subpath(self) -> SubPath {
        match self {
            FittedGeom::Polyline(points) => polyline_subpath(&points),
            FittedGeom::Beziers(chain) => beziers_subpath(&chain),
        }
    }
}

/// Fitting parameters shared by the built-in fitters. Only the spline fitter
/// consults the smoothing/splice fields.
#[derive(Debug, Clone, Copy)]
pub struct FitParams {
    /// Minimum momentary angle (radians) to be considered a corner.
    pub corner_threshold: f64,
    /// Subdivide until all segments are shorter than this length (px).
    pub length_threshold: f64,
    /// Maximum smoothing iterations.
    pub max_iterations: usize,
    /// Minimum angle displacement (radians) to splice a spline.
    pub splice_threshold: f64,
}

impl Default for FitParams {
    fn default() -> Self {
        Self {
            corner_threshold: std::f64::consts::PI / 3.0, // 60°
            length_threshold: 4.0,
            max_iterations: 10,
            splice_threshold: std::f64::consts::PI / 4.0, // 45°
        }
    }
}

/// A curve fitter traces a region mask into closed vector outlines, one
/// [`FittedGeom`] per contour (outer ring or hole).
pub trait CurveFitter {
    fn fit_region(&self, mask: &RegionMask) -> Vec<FittedGeom>;
}

/// Exact lattice polyline; every pixel-boundary step is preserved.
#[derive(Debug, Clone, Default)]
pub struct PixelFitter;

impl CurveFitter for PixelFitter {
    fn fit_region(&self, mask: &RegionMask) -> Vec<FittedGeom> {
        trace_region(mask, PathSimplifyMode::None, FitParams::default())
    }
}

/// Douglas–Peucker polygon with staircase removal.
#[derive(Debug, Clone, Default)]
pub struct PolygonFitter;

impl CurveFitter for PolygonFitter {
    fn fit_region(&self, mask: &RegionMask) -> Vec<FittedGeom> {
        trace_region(mask, PathSimplifyMode::Polygon, FitParams::default())
    }
}

/// Smoothed spline (cubic Bézier) fitter.
#[derive(Debug, Clone, Default)]
pub struct SplineFitter {
    pub params: FitParams,
}

impl SplineFitter {
    pub fn new(params: FitParams) -> Self {
        Self { params }
    }
}

impl CurveFitter for SplineFitter {
    fn fit_region(&self, mask: &RegionMask) -> Vec<FittedGeom> {
        trace_region(mask, PathSimplifyMode::Spline, self.params)
    }
}

/// Trace every connected component of a masked region and collect the
/// resulting outlines, one [`FittedGeom`] per contour, in absolute coordinates.
///
/// This mirrors visioncortex's `Cluster::to_compound_path`: the mask (with
/// holes already punched) is split into connected sub-clusters, each traced
/// independently, then offset into document space.
fn trace_region(mask: &RegionMask, mode: PathSimplifyMode, params: FitParams) -> Vec<FittedGeom> {
    let mut geoms = Vec::new();
    for sub in mask.image.to_clusters(false).iter() {
        let offset = PointI32 {
            x: mask.offset.x + sub.rect.left,
            y: mask.offset.y + sub.rect.top,
        };
        let compound = BinaryCluster::image_to_compound_path(
            &offset,
            &sub.to_binary_image(),
            mode,
            params.corner_threshold,
            params.length_threshold,
            params.max_iterations,
            params.splice_threshold,
        );
        append_compound(&mut geoms, &compound);
    }
    geoms
}

fn append_compound(geoms: &mut Vec<FittedGeom>, compound: &CompoundPath) {
    for element in compound.iter() {
        match element {
            CompoundPathElement::PathI32(p) => {
                let pts: Vec<PointF64> = p
                    .path
                    .iter()
                    .map(|q| PointF64 {
                        x: q.x as f64,
                        y: q.y as f64,
                    })
                    .collect();
                geoms.push(FittedGeom::Polyline(pts));
            }
            CompoundPathElement::PathF64(p) => {
                geoms.push(FittedGeom::Polyline(p.path.clone()));
            }
            CompoundPathElement::Spline(s) => {
                geoms.push(FittedGeom::Beziers(spline_chain(&s.points)));
            }
        }
    }
}

/// A spline of `1 + 3n` points becomes a chain of `n` cubics sharing endpoints.
fn spline_chain(points: &[PointF64]) -> Vec<[PointF64; 4]> {
    if points.len() < 4 || (points.len() - 1) % 3 != 0 {
        return Vec::new();
    }
    let mut chain = Vec::with_capacity((points.len() - 1) / 3);
    let mut start = points[0];
    let mut i = 1;
    while i + 2 < points.len() {
        chain.push([start, points[i], points[i + 1], points[i + 2]]);
        start = points[i + 2];
        i += 3;
    }
    chain
}

/// A closed polyline whose last point repeats the first becomes
/// `MoveTo · LineTo* · Close`.
fn polyline_subpath(points: &[PointF64]) -> SubPath {
    let mut sub = SubPath::new();
    if points.len() < 2 {
        return sub;
    }
    // The tracer emits closed paths whose final point duplicates the first.
    let closed = points.first() == points.last();
    let body_end = if closed { points.len() - 1 } else { points.len() };
    sub.commands.push(PathCmd::MoveTo(points[0]));
    for p in &points[1..body_end] {
        sub.commands.push(PathCmd::LineTo(*p));
    }
    sub.commands.push(PathCmd::Close);
    sub
}

/// A cubic chain becomes `MoveTo · CubicTo* · Close`.
fn beziers_subpath(chain: &[[PointF64; 4]]) -> SubPath {
    let mut sub = SubPath::new();
    if chain.is_empty() {
        return sub;
    }
    sub.commands.push(PathCmd::MoveTo(chain[0][0]));
    for c in chain {
        sub.commands.push(PathCmd::CubicTo(c[1], c[2], c[3]));
    }
    sub.commands.push(PathCmd::Close);
    sub
}

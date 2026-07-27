//! Curve fitters: turn a region's pixel mask into vector outlines.
//!
//! The three built-ins wrap the corresponding visioncortex tracing modes and
//! emit our [`MultiPath`] IR in absolute (document) coordinates:
//!
//! * [`PixelFitter`] — exact lattice polyline (no simplification).
//! * [`PolygonFitter`] — staircase-symmetric Douglas–Peucker polygon.
//! * [`SplineFitter`] — subdivision + corner detection + least-squares cubics.
//!
//! All three trace *closed* region outlines (outer ring plus holes). Open
//! polyline fitting (needed for the mosaic compositor) will arrive with that
//! milestone.

use visioncortex::clusters::Cluster as BinaryCluster;
use visioncortex::{
    CompoundPath, CompoundPathElement, PathSimplifyMode, PointF64, PointI32,
};

use crate::ir::{MultiPath, PathCmd, RegionMask, SubPath};

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

/// A curve fitter traces a region mask into closed vector outlines.
pub trait CurveFitter {
    fn fit_region(&self, mask: &RegionMask) -> MultiPath;
}

/// Exact lattice polyline; every pixel-boundary step is preserved.
#[derive(Debug, Clone, Default)]
pub struct PixelFitter;

impl CurveFitter for PixelFitter {
    fn fit_region(&self, mask: &RegionMask) -> MultiPath {
        trace_region(mask, PathSimplifyMode::None, FitParams::default())
    }
}

/// Douglas–Peucker polygon with staircase removal.
#[derive(Debug, Clone, Default)]
pub struct PolygonFitter;

impl CurveFitter for PolygonFitter {
    fn fit_region(&self, mask: &RegionMask) -> MultiPath {
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
    fn fit_region(&self, mask: &RegionMask) -> MultiPath {
        trace_region(mask, PathSimplifyMode::Spline, self.params)
    }
}

/// Trace every connected component of a masked region and merge the resulting
/// outlines into a single [`MultiPath`] in absolute coordinates.
///
/// This mirrors visioncortex's `Cluster::to_compound_path`: the mask (with
/// holes already punched) is split into connected sub-clusters, each traced
/// independently, then offset into document space.
fn trace_region(mask: &RegionMask, mode: PathSimplifyMode, params: FitParams) -> MultiPath {
    let mut multi = MultiPath::new();
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
        append_compound(&mut multi, &compound);
    }
    multi
}

fn append_compound(multi: &mut MultiPath, compound: &CompoundPath) {
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
                multi.push(polyline_subpath(&pts));
            }
            CompoundPathElement::PathF64(p) => {
                multi.push(polyline_subpath(&p.path));
            }
            CompoundPathElement::Spline(s) => {
                multi.push(spline_subpath(&s.points));
            }
        }
    }
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

/// A spline of `1 + 3n` points becomes `MoveTo · CubicTo* · Close`.
fn spline_subpath(points: &[PointF64]) -> SubPath {
    let mut sub = SubPath::new();
    if points.len() < 4 || (points.len() - 1) % 3 != 0 {
        return sub;
    }
    sub.commands.push(PathCmd::MoveTo(points[0]));
    let mut i = 1;
    while i + 2 < points.len() {
        sub.commands
            .push(PathCmd::CubicTo(points[i], points[i + 1], points[i + 2]));
        i += 3;
    }
    sub.commands.push(PathCmd::Close);
    sub
}

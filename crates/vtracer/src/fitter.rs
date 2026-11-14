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

use visioncortex::{
    BinaryImage, PathI32, PathSimplifyMode, PathWalker, PointF64, PointI32, Spline,
};

use crate::ir::{PathCmd, RegionMask, SubPath};

/// Outset ratio for the 4-point subdivision scheme (matches visioncortex).
const OUTSET_RATIO: f64 = 8.0;

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
pub trait CurveFitter: Send + Sync {
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

/// Find the top-left boundary pixel of a single-component `BinaryImage` without
/// allocating a full boundary `BinaryImage` (unlike `Shape::image_boundary_and_position_length`).
#[inline]
fn find_first_set_pixel(image: &BinaryImage) -> Option<PointI32> {
    let w = image.width;
    let h = image.height;
    for y in 0..h {
        for x in 0..w {
            if image.get_pixel(x, y) {
                return Some(PointI32 {
                    x: x as i32,
                    y: y as i32,
                });
            }
        }
    }
    None
}

#[inline]
fn walk_single_contour(
    image: &BinaryImage,
    clockwise: bool,
    mode: PathSimplifyMode,
    offset: PointI32,
    params: FitParams,
    geoms: &mut Vec<FittedGeom>,
) {
    let Some(start) = find_first_set_pixel(image) else {
        return;
    };
    let walked = PathI32 {
        path: PathWalker::new(image, start, clockwise).collect(),
    };
    if walked.is_empty() {
        return;
    }
    match mode {
        PathSimplifyMode::None => {
            let ox = offset.x as f64;
            let oy = offset.y as f64;
            let pts: Vec<PointF64> = walked
                .path
                .iter()
                .map(|q| PointF64 {
                    x: q.x as f64 + ox,
                    y: q.y as f64 + oy,
                })
                .collect();
            geoms.push(FittedGeom::Polyline(pts));
        }
        PathSimplifyMode::Polygon => {
            let simplified = walked.simplify(clockwise);
            if !simplified.is_empty() {
                let ox = offset.x as f64;
                let oy = offset.y as f64;
                let pts: Vec<PointF64> = simplified
                    .path
                    .iter()
                    .map(|q| PointF64 {
                        x: q.x as f64 + ox,
                        y: q.y as f64 + oy,
                    })
                    .collect();
                geoms.push(FittedGeom::Polyline(pts));
            }
        }
        PathSimplifyMode::Spline => {
            let simplified = walked.simplify(clockwise);
            if simplified.is_empty() {
                return;
            }
            let smoothed = simplified.smooth(
                params.corner_threshold,
                OUTSET_RATIO,
                params.length_threshold,
                params.max_iterations,
            );
            let mut spline = Spline::from_path_f64(&smoothed, params.splice_threshold);
            if !spline.is_empty() {
                spline.offset(&offset.to_point_f64());
                geoms.push(FittedGeom::Beziers(spline_chain(&spline.points)));
            }
        }
    }
}

/// Trace every connected component of a masked region and collect the
/// resulting outlines, one [`FittedGeom`] per contour, in absolute coordinates.
///
/// Uses a fast 4-connected scan without allocating intermediate `Cluster`
/// point vectors or `Shape::image_boundary_and_position_length` bitmaps.
fn trace_region(mask: &RegionMask, mode: PathSimplifyMode, params: FitParams) -> Vec<FittedGeom> {
    let w = mask.image.width;
    let h = mask.image.height;
    let n = w * h;
    if n == 0 {
        return Vec::new();
    }

    // Fast path for a full solid rectangle (e.g. the root canvas layer in
    // watershed or a solid block): 1 component, 0 holes, no flood-fill needed.
    if mask.image.pixels.all() {
        let mut geoms = Vec::with_capacity(1);
        walk_single_contour(&mask.image, true, mode, mask.offset, params, &mut geoms);
        return geoms;
    }

    // Flat byte mirror of `mask.image` (`1` = foreground, `0` = background).
    let mut fg = vec![0u8; n];
    let mut fg_count = 0usize;
    for (i, bit) in mask.image.pixels.iter().enumerate().take(n) {
        if bit {
            fg[i] = 1;
            fg_count += 1;
        }
    }
    if fg_count == 0 {
        return Vec::new();
    }

    let mut geoms = Vec::new();
    let mut visited = vec![0u8; n];
    let mut stack: Vec<usize> = Vec::with_capacity(256);
    let mut comp_pixels: Vec<usize> = Vec::new();
    let mut sub_vis: Vec<u8> = Vec::new();
    let mut hole_pixels: Vec<usize> = Vec::new();

    for start in 0..n {
        if fg[start] == 0 || visited[start] != 0 {
            continue;
        }
        comp_pixels.clear();
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);

        visited[start] = 1;
        stack.push(start);
        while let Some(i) = stack.pop() {
            comp_pixels.push(i);
            let x = i % w;
            let y = i / w;
            if x < x0 {
                x0 = x;
            }
            if y < y0 {
                y0 = y;
            }
            if x > x1 {
                x1 = x;
            }
            if y > y1 {
                y1 = y;
            }
            if x > 0 && fg[i - 1] != 0 && visited[i - 1] == 0 {
                visited[i - 1] = 1;
                stack.push(i - 1);
            }
            if x + 1 < w && fg[i + 1] != 0 && visited[i + 1] == 0 {
                visited[i + 1] = 1;
                stack.push(i + 1);
            }
            if y > 0 && fg[i - w] != 0 && visited[i - w] == 0 {
                visited[i - w] = 1;
                stack.push(i - w);
            }
            if y + 1 < h && fg[i + w] != 0 && visited[i + w] == 0 {
                visited[i + w] = 1;
                stack.push(i + w);
            }
        }

        let cw = x1 - x0 + 1;
        let ch = y1 - y0 + 1;
        let cn = cw * ch;

        // If every cell of the component's bounding box is filled (`comp_pixels.len() == cn`),
        // there are zero background pixels inside the bbox and therefore zero holes!
        if comp_pixels.len() == cn {
            let mut sub_img = BinaryImage {
                pixels: visioncortex::BitVec::from_elem(cn, true),
                width: cw,
                height: ch,
            };
            let sub_offset = PointI32 {
                x: mask.offset.x + x0 as i32,
                y: mask.offset.y + y0 as i32,
            };
            walk_single_contour(&mut sub_img, true, mode, sub_offset, params, &mut geoms);
            continue;
        }

        // Build local byte grid `sub_vis`: `1` = component pixel, `0` = background.
        sub_vis.clear();
        sub_vis.resize(cn, 0);
        let mut sub_img = BinaryImage::new_w_h(cw, ch);
        for &p in &comp_pixels {
            let lx = (p % w) - x0;
            let ly = (p / w) - y0;
            let li = ly * cw + lx;
            sub_vis[li] = 1;
            sub_img.set_pixel_index(li, true);
        }

        // Mark background reachable from the local bbox border as `2` (exterior to this component).
        for lx in 0..cw {
            let top = lx;
            if sub_vis[top] == 0 {
                sub_vis[top] = 2;
                stack.push(top);
            }
            let bot = (ch - 1) * cw + lx;
            if sub_vis[bot] == 0 {
                sub_vis[bot] = 2;
                stack.push(bot);
            }
        }
        for ly in 1..ch.saturating_sub(1) {
            let left = ly * cw;
            if sub_vis[left] == 0 {
                sub_vis[left] = 2;
                stack.push(left);
            }
            let right = ly * cw + cw - 1;
            if sub_vis[right] == 0 {
                sub_vis[right] = 2;
                stack.push(right);
            }
        }
        while let Some(li) = stack.pop() {
            let lx = li % cw;
            let ly = li / cw;
            if lx > 0 && sub_vis[li - 1] == 0 {
                sub_vis[li - 1] = 2;
                stack.push(li - 1);
            }
            if lx + 1 < cw && sub_vis[li + 1] == 0 {
                sub_vis[li + 1] = 2;
                stack.push(li + 1);
            }
            if ly > 0 && sub_vis[li - cw] == 0 {
                sub_vis[li - cw] = 2;
                stack.push(li - cw);
            }
            if ly + 1 < ch && sub_vis[li + cw] == 0 {
                sub_vis[li + cw] = 2;
                stack.push(li + cw);
            }
        }

        // Any remaining `sub_vis[li] == 0` is a true interior hole of THIS component!
        let mut holes: Vec<(BinaryImage, PointI32)> = Vec::new();
        for li_start in 0..cn {
            if sub_vis[li_start] != 0 {
                continue;
            }
            hole_pixels.clear();
            let (mut hx0, mut hy0, mut hx1, mut hy1) = (cw, ch, 0usize, 0usize);
            sub_vis[li_start] = 2;
            stack.push(li_start);
            while let Some(hi) = stack.pop() {
                hole_pixels.push(hi);
                let hx = hi % cw;
                let hy = hi / cw;
                if hx < hx0 {
                    hx0 = hx;
                }
                if hy < hy0 {
                    hy0 = hy;
                }
                if hx > hx1 {
                    hx1 = hx;
                }
                if hy > hy1 {
                    hy1 = hy;
                }
                if hx > 0 && sub_vis[hi - 1] == 0 {
                    sub_vis[hi - 1] = 2;
                    stack.push(hi - 1);
                }
                if hx + 1 < cw && sub_vis[hi + 1] == 0 {
                    sub_vis[hi + 1] = 2;
                    stack.push(hi + 1);
                }
                if hy > 0 && sub_vis[hi - cw] == 0 {
                    sub_vis[hi - cw] = 2;
                    stack.push(hi - cw);
                }
                if hy + 1 < ch && sub_vis[hi + cw] == 0 {
                    sub_vis[hi + cw] = 2;
                    stack.push(hi + cw);
                }
            }

            let hw = hx1 - hx0 + 1;
            let hh = hy1 - hy0 + 1;
            let mut hole_img = BinaryImage::new_w_h(hw, hh);
            for &hp in &hole_pixels {
                let hx = hp % cw;
                let hy = hp / cw;
                sub_img.set_pixel_index(hp, true);
                hole_img.set_pixel_index((hy - hy0) * hw + (hx - hx0), true);
            }
            holes.push((
                hole_img,
                PointI32 {
                    x: mask.offset.x + (x0 + hx0) as i32,
                    y: mask.offset.y + (y0 + hy0) as i32,
                },
            ));
        }

        let sub_offset = PointI32 {
            x: mask.offset.x + x0 as i32,
            y: mask.offset.y + y0 as i32,
        };
        walk_single_contour(&sub_img, true, mode, sub_offset, params, &mut geoms);
        for (hole_img, hole_offset) in &holes {
            walk_single_contour(hole_img, false, mode, *hole_offset, params, &mut geoms);
        }
    }

    geoms
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

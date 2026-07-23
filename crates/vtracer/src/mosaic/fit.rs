//! Stage 3: fit each boundary segment once, with endpoints pinned to nodes.
//!
//! A segment is fitted a single time and cached; both adjacent faces reference
//! the same [`FittedSegment`], one traversed reversed. Reversal is exact, so
//! the shared geometry is bitwise identical and no seam can appear.

use visioncortex::{PointF64, PointI32};

use super::graph::Segment;

/// Fitted geometry for one boundary segment.
#[derive(Clone, Debug)]
pub enum FittedGeom {
    /// Polyline (pixel / polygon backends).
    Polyline(Vec<PointF64>),
    /// Chain of cubic Béziers; consecutive curves share endpoints (spline backend).
    Beziers(Vec<[PointF64; 4]>),
}

/// A fitted segment, cached and indexed by segment id.
#[derive(Clone, Debug)]
pub struct FittedSegment {
    pub geom: FittedGeom,
}

/// Fits a single boundary segment. `fit_open` pins both endpoints (junction
/// nodes must not move); `fit_ring` fits a closed loop with no pinned point.
pub trait SegmentFitter {
    fn fit_open(&self, seg: &Segment) -> FittedSegment;
    fn fit_ring(&self, seg: &Segment) -> FittedSegment;
}

fn to_f64(points: &[PointI32]) -> Vec<PointF64> {
    points
        .iter()
        .map(|p| PointF64 {
            x: p.x as f64,
            y: p.y as f64,
        })
        .collect()
}

/// Identity fitter: lattice points as f64. Produces an exact tessellation and
/// is the reference backend for tests.
#[derive(Debug, Clone, Default)]
pub struct PixelSegmentFitter;

impl SegmentFitter for PixelSegmentFitter {
    fn fit_open(&self, seg: &Segment) -> FittedSegment {
        FittedSegment {
            geom: FittedGeom::Polyline(to_f64(&seg.points)),
        }
    }
    fn fit_ring(&self, seg: &Segment) -> FittedSegment {
        FittedSegment {
            geom: FittedGeom::Polyline(to_f64(&seg.points)),
        }
    }
}

/// Symmetric open Douglas–Peucker. Endpoints are always kept, so junction
/// nodes stay pinned. Plain DP (no directional staircase removal) collapses
/// 1-px staircases to the crack midline — centered between the two regions,
/// which is what a mosaic wants.
#[derive(Debug, Clone)]
pub struct PolygonSegmentFitter {
    pub tolerance: f64,
}

impl Default for PolygonSegmentFitter {
    fn default() -> Self {
        Self { tolerance: 0.5 }
    }
}

impl SegmentFitter for PolygonSegmentFitter {
    fn fit_open(&self, seg: &Segment) -> FittedSegment {
        let pts = to_f64(&seg.points);
        FittedSegment {
            geom: FittedGeom::Polyline(dp_open(&pts, self.tolerance)),
        }
    }

    fn fit_ring(&self, seg: &Segment) -> FittedSegment {
        // Closed loop: split at the vertex farthest from the start, DP each
        // half, then rejoin. points[0] == points[last].
        let pts = to_f64(&seg.points);
        if pts.len() <= 4 {
            return FittedSegment {
                geom: FittedGeom::Polyline(pts),
            };
        }
        let open = &pts[..pts.len() - 1]; // drop duplicate closing point
        let far = farthest_from(open, 0);
        let first: Vec<PointF64> = open[0..=far].to_vec();
        let second: Vec<PointF64> = open[far..]
            .iter()
            .chain(std::iter::once(&open[0]))
            .copied()
            .collect();
        let mut a = dp_open(&first, self.tolerance);
        let b = dp_open(&second, self.tolerance);
        // `a` ends at `far`, `b` starts at `far` and ends back at start.
        a.pop(); // drop shared `far`
        a.extend(b); // ...b includes far..start (closing point == start)
        FittedSegment {
            geom: FittedGeom::Polyline(a),
        }
    }
}

fn farthest_from(pts: &[PointF64], anchor: usize) -> usize {
    let a = pts[anchor];
    let mut best = anchor;
    let mut best_d = -1.0;
    for (i, p) in pts.iter().enumerate() {
        let dx = p.x - a.x;
        let dy = p.y - a.y;
        let d = dx * dx + dy * dy;
        if d > best_d {
            best_d = d;
            best = i;
        }
    }
    best
}

/// Douglas–Peucker on an open polyline; first and last points are always kept.
fn dp_open(pts: &[PointF64], tol: f64) -> Vec<PointF64> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    dp_recurse(pts, 0, pts.len() - 1, tol, &mut keep);
    pts.iter()
        .zip(keep)
        .filter_map(|(p, k)| if k { Some(*p) } else { None })
        .collect()
}

fn dp_recurse(pts: &[PointF64], lo: usize, hi: usize, tol: f64, keep: &mut [bool]) {
    if hi <= lo + 1 {
        return;
    }
    let mut max_d = -1.0;
    let mut idx = lo;
    for i in (lo + 1)..hi {
        let d = perp_distance(pts[i], pts[lo], pts[hi]);
        if d > max_d {
            max_d = d;
            idx = i;
        }
    }
    if max_d > tol {
        keep[idx] = true;
        dp_recurse(pts, lo, idx, tol, keep);
        dp_recurse(pts, idx, hi, tol, keep);
    }
}

/// Perpendicular distance from `p` to the segment `a`–`b`.
fn perp_distance(p: PointF64, a: PointF64, b: PointF64) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return ((p.x - a.x).powi(2) + (p.y - a.y).powi(2)).sqrt();
    }
    let cross = (p.x - a.x) * dy - (p.y - a.y) * dx;
    cross.abs() / len2.sqrt()
}

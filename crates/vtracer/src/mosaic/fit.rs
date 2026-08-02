//! Stage 3: fit each boundary segment once, with endpoints pinned to nodes.
//!
//! A segment is fitted a single time and cached; both adjacent faces reference
//! the same [`FittedSegment`], one traversed reversed. Reversal is exact, so
//! the shared geometry is bitwise identical and no seam can appear.

use visioncortex::{PathI32, PathSimplify, PointF64, PointI32, Spline, SubdivideSmooth};

use super::graph::Segment;

pub use crate::fitter::FittedGeom;

/// Outset ratio for the 4-point subdivision scheme (matches visioncortex).
const OUTSET_RATIO: f64 = 8.0;

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

/// Straight-segment fitter. Uses visioncortex's symmetric `limit_penalties`
/// simplification, which collapses 1px staircases toward the crack midline
/// (centered, no directional outset) so the boundary stays gapless. Endpoints
/// are preserved, pinning junction nodes.
#[derive(Debug, Clone, Default)]
pub struct PolygonSegmentFitter;

impl PolygonSegmentFitter {
    fn fit(&self, seg: &Segment) -> FittedSegment {
        let simplified = PathSimplify::limit_penalties(&PathI32::from_points(seg.points.clone()));
        FittedSegment {
            geom: FittedGeom::Polyline(simplified.path.iter().copied().map(pt).collect()),
        }
    }
}

impl SegmentFitter for PolygonSegmentFitter {
    fn fit_open(&self, seg: &Segment) -> FittedSegment {
        self.fit(seg)
    }
    fn fit_ring(&self, seg: &Segment) -> FittedSegment {
        self.fit(seg)
    }
}

/// Smooth (cubic-Bézier) open-path fitter — the mosaic analogue of the stacked
/// [`crate::fitter::SplineFitter`], but for open segments with pinned
/// endpoints.
///
/// Staircase removal reuses visioncortex's symmetric `limit_penalties`
/// simplification (the same de-noising stacked mode applies), which collapses
/// staircases toward the crack midline. Unlike `remove_staircase`, it has no
/// directional outset, so the boundary stays centered (≤√2/2 px from its
/// crack) and cannot cross a non-adjacent segment — the tessellation stays
/// gapless. A distance-based DP can't do this: near the √2/2 threshold it
/// can't separate staircase noise from real curvature. Smoothing and per-slice
/// cubic fitting then reuse the same visioncortex machinery stacked mode uses
/// (open-path variants of the smoothing primitives + `fit_points_with_beziers`),
/// so the curve character matches stacked.
#[derive(Debug, Clone)]
pub struct SplineSegmentFitter {
    /// Corner angle threshold, radians.
    pub corner_threshold: f64,
    /// Subdivide until segments are shorter than this (px).
    pub length_threshold: f64,
    pub max_iterations: usize,
    /// Splice angle threshold, radians.
    pub splice_threshold: f64,
}

impl Default for SplineSegmentFitter {
    fn default() -> Self {
        Self {
            corner_threshold: std::f64::consts::PI / 3.0,
            length_threshold: 4.0,
            max_iterations: 10,
            splice_threshold: std::f64::consts::PI / 4.0,
        }
    }
}

fn pt(p: PointI32) -> PointF64 {
    PointF64 {
        x: p.x as f64,
        y: p.y as f64,
    }
}

/// A degenerate cubic tracing the straight line `a`→`b`.
fn straight_cubic(a: PointF64, b: PointF64) -> [PointF64; 4] {
    let c1 = PointF64 {
        x: a.x + (b.x - a.x) / 3.0,
        y: a.y + (b.y - a.y) / 3.0,
    };
    let c2 = PointF64 {
        x: a.x + 2.0 * (b.x - a.x) / 3.0,
        y: a.y + 2.0 * (b.y - a.y) / 3.0,
    };
    [a, c1, c2, b]
}

/// Error bound for the per-slice cubic fit. Matches the value stacked mode
/// uses in `Spline::from_path_f64`, so mosaic curves have the same character.
const FIT_ERROR: f64 = 10.0;

/// Fit one splice slice, exactly as stacked mode does
/// (`fit_points_with_beziers`: the full retract-handled cubic chain per slice,
/// outer endpoints pinned to the slice ends — a sparse or multi-curve slice is
/// kept faithful instead of being collapsed onto one ballooning cubic).
fn fit_slice(slice: &[PointF64], out: &mut Vec<[PointF64; 4]>) {
    match slice.len() {
        0 | 1 => {}
        2 => out.push(straight_cubic(slice[0], slice[1])),
        _ => out.extend(SubdivideSmooth::fit_points_with_beziers(slice, FIT_ERROR)),
    }
}

fn spline_to_beziers(spline: &Spline) -> Vec<[PointF64; 4]> {
    spline
        .get_control_points()
        .into_iter()
        .filter(|w| w.len() == 4)
        .map(|w| [w[0], w[1], w[2], w[3]])
        .collect()
}

impl SegmentFitter for SplineSegmentFitter {
    fn fit_open(&self, seg: &Segment) -> FittedSegment {
        if seg.points.len() <= 2 {
            return FittedSegment {
                geom: FittedGeom::Polyline(to_f64(&seg.points)),
            };
        }

        // 1. Staircase removal via visioncortex's `limit_penalties` — the
        //    symmetric (area-based, no directional outset) simplifier stacked
        //    mode runs after remove_staircase. Used alone here it collapses
        //    staircases toward the crack midline, so the boundary stays
        //    centered and cannot cross a non-adjacent segment (which would
        //    open a gap in the tessellation). Endpoints are preserved.
        let simplified = PathSimplify::limit_penalties(&PathI32::from_points(seg.points.clone()));
        if simplified.len() <= 2 {
            return FittedSegment {
                geom: FittedGeom::Polyline(simplified.path.iter().copied().map(pt).collect()),
            };
        }

        // 2. Corner detection (open, endpoints forced as corners).
        let mut corners = SubdivideSmooth::find_corners(&simplified, self.corner_threshold, false);
        // 3. Open 4-point subdivision.
        let mut path = simplified.to_path_f64();
        for _ in 0..self.max_iterations {
            let (np, nc, done) = SubdivideSmooth::subdivide_keep_corners(
                &path,
                &corners,
                OUTSET_RATIO,
                self.length_threshold,
                false,
            );
            path = np;
            corners = nc;
            if done {
                break;
            }
        }
        // 4. Splice points (open, endpoints forced).
        let splice = SubdivideSmooth::find_splice_points(&path, self.splice_threshold, false);
        let cuts: Vec<usize> = splice
            .iter()
            .enumerate()
            .filter_map(|(i, &s)| if s { Some(i) } else { None })
            .collect();

        // 5. Per-slice cubic fit.
        let mut beziers = Vec::new();
        for w in cuts.windows(2) {
            fit_slice(&path.path[w[0]..=w[1]], &mut beziers);
        }

        if beziers.is_empty() {
            return FittedSegment {
                geom: FittedGeom::Polyline(path.path.clone()),
            };
        }

        // Pin the segment's endpoints exactly to the lattice nodes so that
        // segments meeting at a junction share identical coordinates.
        beziers.first_mut().unwrap()[0] = pt(seg.points[0]);
        beziers.last_mut().unwrap()[3] = pt(seg.points[seg.points.len() - 1]);

        FittedSegment {
            geom: FittedGeom::Beziers(beziers),
        }
    }

    fn fit_ring(&self, seg: &Segment) -> FittedSegment {
        // Rings are closed loops — this is exactly the stacked closed-spline
        // pipeline (simplify → smooth → fit).
        if seg.points.len() <= 4 {
            return FittedSegment {
                geom: FittedGeom::Polyline(to_f64(&seg.points)),
            };
        }
        let simplified = PathSimplify::limit_penalties(&PathI32::from_points(seg.points.clone()));
        let smoothed = simplified.smooth(
            self.corner_threshold,
            OUTSET_RATIO,
            self.length_threshold,
            self.max_iterations,
        );
        let spline = Spline::from_path_f64(&smoothed, self.splice_threshold);
        let beziers = spline_to_beziers(&spline);
        if beziers.is_empty() {
            return FittedSegment {
                geom: FittedGeom::Polyline(to_f64(&seg.points)),
            };
        }
        FittedSegment {
            geom: FittedGeom::Beziers(beziers),
        }
    }
}

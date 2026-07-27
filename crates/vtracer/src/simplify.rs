//! Curve passes: geometry transforms between curve fitting and composition.
//!
//! A [`CurvePass`] rewrites one fitted contour at a time. Passes run *before*
//! composition on the fitted geometry itself — in mosaic mode each shared
//! boundary segment is transformed exactly once and both adjacent faces
//! reference the result, so the tessellation stays seam-free by construction.
//! Running instead on the composed [`VectorDoc`](crate::ir::VectorDoc) would
//! re-fit the two copies of every shared boundary independently and reopen
//! the seams the mosaic exists to prevent.
//!
//! The built-in pass is [`SimplifyCurves`], the paper.js `simplify` analogue.

use flo_curves::bezier::{fit_curve_cubic, Curve};
use flo_curves::Coord2;
use visioncortex::PointF64;

use crate::fitter::FittedGeom;

/// A geometry pass over one fitted contour, run between curve fitting and
/// composition. Implementations must keep an open chain's endpoints exactly
/// (mosaic junction nodes must not move) and keep a ring closed.
pub trait CurvePass {
    /// Transform an open chain; both endpoints are pinned.
    fn open(&self, geom: FittedGeom) -> FittedGeom;
    /// Transform a closed ring.
    fn ring(&self, geom: FittedGeom) -> FittedGeom;
}

/// paper.js-style curve simplification (Schneider's fit): re-fit each smooth
/// run of cubics between corners with the fewest curves that stay within
/// `tolerance` of the fitted geometry.
///
/// The spline fitters cut an outline at every splice point and fit each short
/// slice separately, so a lazily curving edge carries an anchor per splice.
/// This pass samples the fitted curve (~1 px spacing) and re-fits whole
/// corner-to-corner runs with `flo_curves`' Schneider implementation
/// (`fit_curve_cubic`, tangents taken from the chain's own ends), merging
/// those slices down to what the tolerance genuinely requires.
///
/// A run is replaced only when the re-fit uses strictly fewer cubics and is
/// kept verbatim otherwise, so the pass never increases the curve count and
/// never moves the geometry more than `tolerance` (measured at the samples).
/// Polylines (pixel / polygon modes) pass through untouched.
#[derive(Debug, Clone, Copy)]
pub struct SimplifyCurves {
    /// Maximum distance (px) the simplified curve may stray from the fitted
    /// one. paper.js defaults to 2.5.
    pub tolerance: f64,
    /// Tangent-break angle (radians) above which an anchor is a corner and
    /// must survive in place; runs are re-fitted between corners.
    pub corner_threshold: f64,
}

impl CurvePass for SimplifyCurves {
    fn open(&self, geom: FittedGeom) -> FittedGeom {
        match geom {
            FittedGeom::Beziers(chain) => FittedGeom::Beziers(self.simplify_chain(chain, false)),
            other => other,
        }
    }

    fn ring(&self, geom: FittedGeom) -> FittedGeom {
        match geom {
            FittedGeom::Beziers(chain) => FittedGeom::Beziers(self.simplify_chain(chain, true)),
            other => other,
        }
    }
}

impl SimplifyCurves {
    fn simplify_chain(&self, mut chain: Vec<[PointF64; 4]>, closed: bool) -> Vec<[PointF64; 4]> {
        if self.tolerance <= 0.0 || chain.len() < 2 {
            return chain;
        }

        if closed {
            // The re-fit pins run endpoints, so a ring needs a seam. Put it at
            // the sharpest junction (wraparound included): a corner the fit
            // would keep anyway, or the least-smooth anchor when the ring has
            // none, so any residual tangent break lands where it hides best.
            let angles: Vec<f64> = (0..chain.len())
                .map(|k| {
                    let prev = if k == 0 { chain.len() - 1 } else { k - 1 };
                    break_angle(&chain[prev], &chain[k])
                })
                .collect();
            let seam = angles
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0);
            chain.rotate_left(seam);
        }

        // Cut into smooth runs at corner anchors (chain ends are always cuts).
        let mut cuts: Vec<usize> = vec![0];
        for i in 1..chain.len() {
            if break_angle(&chain[i - 1], &chain[i]) >= self.corner_threshold {
                cuts.push(i);
            }
        }
        cuts.push(chain.len());

        let mut out: Vec<[PointF64; 4]> = Vec::with_capacity(chain.len());
        for w in cuts.windows(2) {
            let run = &chain[w[0]..w[1]];
            if run.len() < 2 {
                out.extend_from_slice(run);
                continue;
            }
            match refit_run(run, self.tolerance) {
                Some(refit) if refit.len() < run.len() => out.extend(refit),
                _ => out.extend_from_slice(run),
            }
        }
        out
    }
}

/// Schneider-fit one smooth run: sample it, then `fit_curve_cubic` with the
/// run's own end tangents (`end_tangent` points backward, per its contract).
/// The recursion splits at sample points, so consecutive fitted cubics share
/// endpoints exactly; the outer endpoints are pinned to the run's, bit for
/// bit. Returns `None` for degenerate (point-like) runs.
fn refit_run(run: &[[PointF64; 4]], tolerance: f64) -> Option<Vec<[PointF64; 4]>> {
    let start_tan = tangent_out(run.first()?)?;
    let end_tan = tangent_in(run.last()?)?;
    let samples: Vec<Coord2> = sample_run(run, tolerance)
        .into_iter()
        .map(|p| Coord2(p.x, p.y))
        .collect();

    let fitted: Vec<Curve<Coord2>> = fit_curve_cubic(
        &samples,
        &Coord2(start_tan.0, start_tan.1),
        &Coord2(-end_tan.0, -end_tan.1),
        tolerance,
    );
    if fitted.is_empty() {
        return None;
    }

    let pt = |c: Coord2| PointF64 { x: c.0, y: c.1 };
    let mut out: Vec<[PointF64; 4]> = fitted
        .into_iter()
        .map(|c| [pt(c.start_point), pt(c.control_points.0), pt(c.control_points.1), pt(c.end_point)])
        .collect();
    out.first_mut()?[0] = run[0][0];
    out.last_mut()?[3] = run[run.len() - 1][3];
    Some(out)
}

fn dist(a: PointF64, b: PointF64) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

/// Unit direction a→b, or `None` when the points (nearly) coincide.
fn dir(a: PointF64, b: PointF64) -> Option<(f64, f64)> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-9 {
        None
    } else {
        Some((dx / len, dy / len))
    }
}

/// Tangent arriving at a cubic's end: the last distinct control point wins.
fn tangent_in(c: &[PointF64; 4]) -> Option<(f64, f64)> {
    dir(c[2], c[3]).or_else(|| dir(c[1], c[3])).or_else(|| dir(c[0], c[3]))
}

/// Tangent leaving a cubic's start: the first distinct control point wins.
fn tangent_out(c: &[PointF64; 4]) -> Option<(f64, f64)> {
    dir(c[0], c[1]).or_else(|| dir(c[0], c[2])).or_else(|| dir(c[0], c[3]))
}

/// Turn angle at the junction of two consecutive cubics. A fully degenerate
/// (point-like) neighbour counts as a corner so it is never smoothed across.
fn break_angle(prev: &[PointF64; 4], next: &[PointF64; 4]) -> f64 {
    match (tangent_in(prev), tangent_out(next)) {
        (Some(a), Some(b)) => (a.0 * b.0 + a.1 * b.1).clamp(-1.0, 1.0).acos(),
        _ => std::f64::consts::PI,
    }
}

fn cubic_at(c: &[PointF64; 4], t: f64) -> PointF64 {
    let u = 1.0 - t;
    let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    PointF64 {
        x: b0 * c[0].x + b1 * c[1].x + b2 * c[2].x + b3 * c[3].x,
        y: b0 * c[0].y + b1 * c[1].y + b2 * c[2].y + b3 * c[3].y,
    }
}

/// Sample a run of cubics at roughly 1 px spacing (by control-polygon length),
/// tighter when the tolerance is sub-pixel — the fit measures its error at
/// the samples, so their spacing is the fidelity guard. The first and last
/// samples are the run's endpoints, exactly: `cubic_at` with `t = 1` returns
/// `c[3]` bit for bit.
fn sample_run(run: &[[PointF64; 4]], tolerance: f64) -> Vec<PointF64> {
    let spacing = tolerance.clamp(0.25, 1.0);
    let mut samples = vec![run[0][0]];
    for c in run {
        let len = dist(c[0], c[1]) + dist(c[1], c[2]) + dist(c[2], c[3]);
        let n = ((len / spacing).ceil() as usize).clamp(1, 512);
        for k in 1..=n {
            samples.push(cubic_at(c, k as f64 / n as f64));
        }
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f64, y: f64) -> PointF64 {
        PointF64 { x, y }
    }

    /// A degenerate cubic tracing the straight line `a`→`b`.
    fn straight(a: PointF64, b: PointF64) -> [PointF64; 4] {
        let lerp = |t: f64| pt(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
        [a, lerp(1.0 / 3.0), lerp(2.0 / 3.0), b]
    }

    /// `n` straight cubics subdividing the segment `a`→`b`.
    fn straight_chain(a: PointF64, b: PointF64, n: usize) -> Vec<[PointF64; 4]> {
        let lerp = |t: f64| pt(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
        (0..n)
            .map(|i| straight(lerp(i as f64 / n as f64), lerp((i + 1) as f64 / n as f64)))
            .collect()
    }

    /// One cubic approximating the circular arc `a0..a1` on a circle of
    /// radius `r` about the origin (the classic 4/3·tan(Δ/4) handle length).
    fn arc_cubic(r: f64, a0: f64, a1: f64) -> [PointF64; 4] {
        let k = 4.0 / 3.0 * ((a1 - a0) / 4.0).tan();
        let (p0, p3) = (pt(r * a0.cos(), r * a0.sin()), pt(r * a1.cos(), r * a1.sin()));
        [
            p0,
            pt(p0.x - k * r * a0.sin(), p0.y + k * r * a0.cos()),
            pt(p3.x + k * r * a1.sin(), p3.y - k * r * a1.cos()),
            p3,
        ]
    }

    fn pass() -> SimplifyCurves {
        SimplifyCurves {
            tolerance: 1.0,
            corner_threshold: std::f64::consts::PI / 3.0,
        }
    }

    fn anchors(chain: &[[PointF64; 4]]) -> Vec<PointF64> {
        let mut a: Vec<PointF64> = chain.iter().map(|c| c[0]).collect();
        a.push(chain.last().unwrap()[3]);
        a
    }

    #[test]
    fn collinear_run_collapses_to_one_cubic() {
        let chain = straight_chain(pt(0.0, 0.0), pt(100.0, 0.0), 10);
        let out = match pass().open(FittedGeom::Beziers(chain)) {
            FittedGeom::Beziers(c) => c,
            _ => panic!("geometry kind changed"),
        };
        assert_eq!(out.len(), 1, "ten collinear cubics become one");
        assert_eq!(out[0][0], pt(0.0, 0.0), "start pinned");
        assert_eq!(out[0][3], pt(100.0, 0.0), "end pinned");
    }

    #[test]
    fn corner_survives_in_place() {
        // An L: two straight runs meeting at a right angle.
        let mut chain = straight_chain(pt(0.0, 0.0), pt(50.0, 0.0), 5);
        chain.extend(straight_chain(pt(50.0, 0.0), pt(50.0, 50.0), 5));
        let out = match pass().open(FittedGeom::Beziers(chain)) {
            FittedGeom::Beziers(c) => c,
            _ => panic!("geometry kind changed"),
        };
        assert_eq!(out.len(), 2, "one cubic per leg");
        assert_eq!(out[0][3], pt(50.0, 0.0), "corner anchor exact");
        assert_eq!(out[1][0], pt(50.0, 0.0), "chain continuous through corner");
        assert_eq!(out[0][0], pt(0.0, 0.0));
        assert_eq!(out[1][3], pt(50.0, 50.0));
    }

    #[test]
    fn ring_stays_closed_and_keeps_square_corners() {
        // A closed square, three cubics per side, seam mid-side (anchor 0 is
        // smooth) — the pass must rotate the seam onto a corner.
        let corners = [pt(0.0, 0.0), pt(60.0, 0.0), pt(60.0, 60.0), pt(0.0, 60.0)];
        let mut chain = Vec::new();
        for i in 0..4 {
            chain.extend(straight_chain(corners[i], corners[(i + 1) % 4], 3));
        }
        chain.rotate_left(1); // seam mid-side
        let out = match pass().ring(FittedGeom::Beziers(chain)) {
            FittedGeom::Beziers(c) => c,
            _ => panic!("geometry kind changed"),
        };
        assert_eq!(out.len(), 4, "one cubic per side");
        assert_eq!(out[0][0], out.last().unwrap()[3], "ring closed");
        let mut got = anchors(&out);
        got.pop(); // last repeats first
        for c in corners {
            assert!(got.contains(&c), "corner {c:?} kept, got {got:?}");
        }
    }

    #[test]
    fn arc_merges_within_tolerance() {
        // A quarter circle as 8 short arcs collapses to far fewer cubics, and
        // the result stays within tolerance of the true circle.
        let r = 50.0;
        let n = 8;
        let chain: Vec<[PointF64; 4]> = (0..n)
            .map(|i| {
                let step = std::f64::consts::FRAC_PI_2 / n as f64;
                arc_cubic(r, i as f64 * step, (i + 1) as f64 * step)
            })
            .collect();
        let tol = 0.5;
        let p = SimplifyCurves {
            tolerance: tol,
            corner_threshold: std::f64::consts::PI / 3.0,
        };
        let out = match p.open(FittedGeom::Beziers(chain)) {
            FittedGeom::Beziers(c) => c,
            _ => panic!("geometry kind changed"),
        };
        assert!(out.len() < 8, "arcs merge, got {}", out.len());
        for c in &out {
            for k in 0..=32 {
                let q = cubic_at(c, k as f64 / 32.0);
                let radial = ((q.x * q.x + q.y * q.y).sqrt() - r).abs();
                assert!(radial <= tol + 0.1, "deviation {radial} beyond tolerance");
            }
        }
    }

    #[test]
    fn refit_never_grows_the_chain() {
        // A single cubic is untouchable; a sharp S of two cubics that cannot
        // merge within a tiny tolerance is kept verbatim.
        let lone = vec![arc_cubic(50.0, 0.0, 1.0)];
        match pass().open(FittedGeom::Beziers(lone.clone())) {
            FittedGeom::Beziers(c) => assert_eq!(c, lone),
            _ => panic!("geometry kind changed"),
        }

        let s_curve = vec![
            [pt(0.0, 0.0), pt(20.0, 40.0), pt(30.0, 40.0), pt(50.0, 0.0)],
            [pt(50.0, 0.0), pt(70.0, -40.0), pt(80.0, -40.0), pt(100.0, 0.0)],
        ];
        let tight = SimplifyCurves {
            tolerance: 0.01,
            corner_threshold: std::f64::consts::PI / 3.0,
        };
        match tight.open(FittedGeom::Beziers(s_curve.clone())) {
            FittedGeom::Beziers(c) => {
                assert!(c.len() <= s_curve.len(), "never more cubics than input")
            }
            _ => panic!("geometry kind changed"),
        }
    }

    #[test]
    fn polylines_pass_through_untouched() {
        let poly = vec![pt(0.0, 0.0), pt(1.0, 0.0), pt(2.0, 0.0), pt(3.0, 0.0)];
        match pass().open(FittedGeom::Polyline(poly.clone())) {
            FittedGeom::Polyline(p) => assert_eq!(p, poly),
            _ => panic!("polyline must stay a polyline"),
        }
        match pass().ring(FittedGeom::Polyline(poly.clone())) {
            FittedGeom::Polyline(p) => assert_eq!(p, poly),
            _ => panic!("polyline must stay a polyline"),
        }
    }
}

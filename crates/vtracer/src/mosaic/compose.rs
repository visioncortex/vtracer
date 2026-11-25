//! Stage 4: compose per-region SVG paths from shared fitted segments.
//!
//! Each region becomes one shape whose `d` concatenates its contours as
//! subpaths (default `nonzero` fill rule handles holes and pinch points). Each
//! oriented segment is emitted skipping its first point (identical to the
//! previous segment's last point), so shared boundaries are byte-identical on
//! both sides.

use crate::ir::{MultiPath, PathCmd, Shape, SubPath, VectorDoc};
use crate::simplify::CurvePass;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

use super::face::{assemble, Contour, Face};
use super::fit::{FittedGeom, FittedSegment, SegmentFitter};
use super::graph::BoundaryGraph;
use super::{LabelMap, Segmentation};

/// Run the full mosaic pipeline: flatten → merge similar neighbours →
/// boundary graph → faces → fit → curve passes → compose.
///
/// `merge_diff` is the color-difference threshold for
/// [`LabelMap::merge_similar`]; pass the clustering `deepen_diff`
/// (gradient step) so the flattened mosaic rejoins what only the stacked
/// gradient layering had split. `0` still merges identical-color
/// neighbours; negative disables merging entirely.
///
/// `passes` run on each fitted segment before composition — once per shared
/// boundary, so both adjacent faces reference the transformed geometry and
/// the tessellation stays seam-free.
pub fn compose_mosaic(
    seg: &Segmentation,
    fitter: &dyn SegmentFitter,
    merge_diff: i32,
    passes: &[Box<dyn CurvePass>],
) -> VectorDoc {
    let mut map = LabelMap::from_segmentation(seg);
    map.merge_similar(merge_diff);
    let graph = BoundaryGraph::extract(&map);
    let faces = assemble(&graph, &map);

    let fit_one = |s: &super::graph::Segment| {
        let ring = s.is_ring();
        let mut geom = if ring {
            fitter.fit_ring(s).geom
        } else {
            fitter.fit_open(s).geom
        };
        for pass in passes {
            geom = if ring { pass.ring(geom) } else { pass.open(geom) };
        }
        FittedSegment { geom }
    };

    // Fit every segment exactly once; both adjacent faces share the result.
    #[cfg(not(target_arch = "wasm32"))]
    let fitted: Vec<FittedSegment> = graph.segments.par_iter().map(fit_one).collect();
    #[cfg(target_arch = "wasm32")]
    let fitted: Vec<FittedSegment> = graph.segments.iter().map(fit_one).collect();

    let mut doc = VectorDoc::new(seg.width, seg.height);
    doc.shapes.reserve(faces.len());
    for face in &faces {
        let path = build_path(face, &fitted, &graph);
        if !path.is_empty() {
            doc.shapes.push(Shape {
                paint: map.paints[face.region as usize],
                path,
            });
        }
    }
    doc
}

fn build_path(face: &Face, fitted: &[FittedSegment], _graph: &BoundaryGraph) -> MultiPath {
    let mut mp = MultiPath::new();
    for contour in &face.contours {
        let mut sub = SubPath::new();
        emit_contour(contour, fitted, &mut sub);
        if !sub.is_empty() {
            sub.commands.push(PathCmd::Close);
            mp.subpaths.push(sub);
        }
    }
    mp
}

fn emit_contour(contour: &Contour, fitted: &[FittedSegment], sub: &mut SubPath) {
    for (i, sref) in contour.0.iter().enumerate() {
        let geom = &fitted[sref.seg as usize].geom;
        emit_segment(geom, sref.forward, i == 0, sub);
    }
}

/// Append one oriented segment's commands. When `first`, opens with a `MoveTo`;
/// otherwise the leading point (shared with the previous segment) is skipped.
fn emit_segment(geom: &FittedGeom, forward: bool, first: bool, sub: &mut SubPath) {
    match geom {
        FittedGeom::Polyline(pts) => {
            if pts.len() < 2 {
                return;
            }
            if forward {
                if first {
                    sub.commands.push(PathCmd::MoveTo(pts[0]));
                }
                for p in &pts[1..] {
                    sub.commands.push(PathCmd::LineTo(*p));
                }
            } else {
                if first {
                    sub.commands.push(PathCmd::MoveTo(pts[pts.len() - 1]));
                }
                for p in pts[..pts.len() - 1].iter().rev() {
                    sub.commands.push(PathCmd::LineTo(*p));
                }
            }
        }
        FittedGeom::Beziers(curves) => {
            if curves.is_empty() {
                return;
            }
            // Reversing a cubic is exact: [p0,p1,p2,p3] -> [p3,p2,p1,p0], and
            // the whole chain reverses in order too.
            if forward {
                if first {
                    sub.commands.push(PathCmd::MoveTo(curves[0][0]));
                }
                for c in curves {
                    sub.commands.push(PathCmd::CubicTo(c[1], c[2], c[3]));
                }
            } else {
                if first {
                    sub.commands.push(PathCmd::MoveTo(curves[curves.len() - 1][3]));
                }
                for c in curves.iter().rev() {
                    sub.commands.push(PathCmd::CubicTo(c[2], c[1], c[0]));
                }
            }
        }
    }
}

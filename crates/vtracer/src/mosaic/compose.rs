//! Stage 4: compose per-region SVG paths from shared fitted segments.
//!
//! Each region becomes one shape whose `d` concatenates its contours as
//! subpaths (default `nonzero` fill rule handles holes and pinch points). Each
//! oriented segment is emitted skipping its first point (identical to the
//! previous segment's last point), so shared boundaries are byte-identical on
//! both sides.

use crate::ir::{MultiPath, PathCmd, Shape, SubPath, VectorDoc};
use visioncortex::PointF64;

use super::face::{assemble, Contour, Face};
use super::fit::{FittedGeom, FittedSegment, SegmentFitter};
use super::graph::BoundaryGraph;
use super::{LabelMap, Segmentation};

/// Run the full mosaic pipeline: flatten → boundary graph → faces → fit → compose.
pub fn compose_mosaic(seg: &Segmentation, fitter: &dyn SegmentFitter) -> VectorDoc {
    let map = LabelMap::from_segmentation(seg);
    let graph = BoundaryGraph::extract(&map);
    let faces = assemble(&graph, &map);

    // Fit every segment exactly once; both adjacent faces share the result.
    let fitted: Vec<FittedSegment> = graph
        .segments
        .iter()
        .map(|s| {
            if s.is_ring() {
                fitter.fit_ring(s)
            } else {
                fitter.fit_open(s)
            }
        })
        .collect();

    let mut doc = VectorDoc::new(seg.width, seg.height);
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
            let ordered: Vec<PointF64> = if forward {
                pts.clone()
            } else {
                pts.iter().rev().copied().collect()
            };
            if first {
                sub.commands.push(PathCmd::MoveTo(ordered[0]));
            }
            for p in &ordered[1..] {
                sub.commands.push(PathCmd::LineTo(*p));
            }
        }
        FittedGeom::Beziers(curves) => {
            if curves.is_empty() {
                return;
            }
            // Reversing a cubic is exact: [p0,p1,p2,p3] -> [p3,p2,p1,p0], and
            // the whole chain reverses in order too.
            let ordered: Vec<[PointF64; 4]> = if forward {
                curves.clone()
            } else {
                curves
                    .iter()
                    .rev()
                    .map(|c| [c[3], c[2], c[1], c[0]])
                    .collect()
            };
            if first {
                sub.commands.push(PathCmd::MoveTo(ordered[0][0]));
            }
            for c in &ordered {
                sub.commands.push(PathCmd::CubicTo(c[1], c[2], c[3]));
            }
        }
    }
}

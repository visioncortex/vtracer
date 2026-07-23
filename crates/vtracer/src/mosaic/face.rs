//! Stage 2: face assembly.
//!
//! Lift the "region kept on the left" successor rule from unit edges to whole
//! segments. Following it around each region yields its contours; because the
//! interior is always on the left, outer contours and hole contours come out
//! with opposite winding automatically — no containment/nesting computation is
//! needed, and the region can be filled with a single `nonzero` path.

use super::graph::{
    edge_present, left_pixel_at, reverse, straight, turn_left, turn_right, BoundaryGraph, SegRef,
};
use super::{LabelMap, RegionId, OUTSIDE};

/// A closed cycle of directed segments bounding (part of) a region.
#[derive(Clone, Debug)]
pub struct Contour(pub Vec<SegRef>);

/// One region and all of its contours (outer + holes).
#[derive(Clone, Debug)]
pub struct Face {
    pub region: RegionId,
    pub contours: Vec<Contour>,
}

/// Left region of a directed segment view.
fn left_region(graph: &BoundaryGraph, r: SegRef) -> RegionId {
    let seg = &graph.segments[r.seg as usize];
    if r.forward {
        seg.left
    } else {
        seg.right
    }
}

/// Pick the next unit direction leaving `corner`, keeping region `r` on the
/// left: sharpest right turn first (this pinches checkerboard nodes and keeps
/// contours simple).
fn successor(map: &LabelMap, x: i32, y: i32, d_in: u8, r: RegionId) -> u8 {
    for &d in &[turn_right(d_in), straight(d_in), turn_left(d_in)] {
        if edge_present(map, x, y, d) && left_pixel_at(map, x, y, d) == r {
            return d;
        }
    }
    unreachable!("no successor edge keeps the region on the left");
}

pub fn assemble(graph: &BoundaryGraph, map: &LabelMap) -> Vec<Face> {
    let mut by_region: Vec<Vec<Contour>> = vec![Vec::new(); map.paints.len()];
    // usage[seg][0] = forward view used, [1] = backward view used.
    let mut used = vec![[false; 2]; graph.segments.len()];

    for seg_id in 0..graph.segments.len() {
        if graph.segments[seg_id].is_ring() {
            continue;
        }
        for &forward in &[true, false] {
            let start = SegRef {
                seg: seg_id as u32,
                forward,
            };
            let region = left_region(graph, start);
            if region == OUTSIDE || used[seg_id][forward as usize] {
                continue;
            }

            let mut contour = Vec::new();
            let mut cur = start;
            loop {
                used[cur.seg as usize][cur.forward as usize] = true;
                contour.push(cur);

                let seg = &graph.segments[cur.seg as usize];
                let (node_id, d_in) = if cur.forward {
                    (seg.end.unwrap(), seg.last_dir)
                } else {
                    (seg.start.unwrap(), reverse(seg.first_dir))
                };
                let corner = graph.nodes[node_id as usize].corner;
                let d_next = successor(map, corner.x, corner.y, d_in, region);
                cur = graph.nodes[node_id as usize].out[d_next as usize]
                    .expect("successor direction must have an outgoing segment");

                if cur == start {
                    break;
                }
            }
            if (region as usize) < by_region.len() {
                by_region[region as usize].push(Contour(contour));
            }
        }
    }

    // Rings: the left side uses it forward, the right side reversed.
    for seg_id in 0..graph.segments.len() {
        let seg = &graph.segments[seg_id];
        if !seg.is_ring() {
            continue;
        }
        if seg.left != OUTSIDE && (seg.left as usize) < by_region.len() {
            by_region[seg.left as usize].push(Contour(vec![SegRef {
                seg: seg_id as u32,
                forward: true,
            }]));
        }
        if seg.right != OUTSIDE && (seg.right as usize) < by_region.len() {
            by_region[seg.right as usize].push(Contour(vec![SegRef {
                seg: seg_id as u32,
                forward: false,
            }]));
        }
    }

    by_region
        .into_iter()
        .enumerate()
        .filter(|(_, c)| !c.is_empty())
        .map(|(region, contours)| Face {
            region: region as RegionId,
            contours,
        })
        .collect()
}

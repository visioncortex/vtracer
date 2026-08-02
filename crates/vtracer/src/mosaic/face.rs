//! Stage 2: face assembly.
//!
//! Lift the "region kept on the left" successor rule from unit edges to whole
//! segments. Following it around each region yields its contours; because the
//! interior is always on the left, outer contours and hole contours come out
//! with opposite winding automatically — no containment/nesting computation is
//! needed, and the region can be filled with a single `nonzero` path.

use std::collections::BTreeMap;

use super::graph::{
    dir_from_delta, edge_present, left_pixel_at, left_pixel_coord, reverse, straight, turn_left,
    turn_right, BoundaryGraph, SegRef,
};
use super::{LabelMap, RegionId, OUTSIDE};

/// Island id for pixels that belong to no region.
const NO_ISLAND: u32 = u32::MAX;

/// A closed cycle of directed segments bounding (part of) a region.
#[derive(Clone, Debug)]
pub struct Contour(pub Vec<SegRef>);

/// One connected patch of a region and all of its contours (outer + holes).
///
/// A region can appear as several disjoint patches; each gets its own face, so
/// isolated islands never share a path.
#[derive(Clone, Debug)]
pub struct Face {
    pub region: RegionId,
    pub contours: Vec<Contour>,
}

/// Connected-component ("island") id per pixel, grouping equal labels with
/// 8-connectivity. [`OUTSIDE`] pixels get [`NO_ISLAND`].
///
/// 8-connectivity is what matches [`successor`]: it pinches a checkerboard
/// corner into one contour, so two lobes meeting only at a diagonal are walked
/// as a single contour and must land in a single face. Splitting them
/// (4-connectivity) could put a hole contour in a different face than the ring
/// enclosing it, and a lone hole ring fills solid under `nonzero`.
fn islands(map: &LabelMap) -> Vec<u32> {
    let (w, h) = (map.width as usize, map.height as usize);
    let mut ids = vec![NO_ISLAND; w * h];
    let mut next = 0u32;
    let mut stack: Vec<(usize, usize)> = Vec::new();

    for start in 0..w * h {
        if ids[start] != NO_ISLAND || map.labels[start] == OUTSIDE {
            continue;
        }
        let label = map.labels[start];
        let id = next;
        next += 1;
        ids[start] = id;
        stack.push((start % w, start / w));

        while let Some((x, y)) = stack.pop() {
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let n = ny as usize * w + nx as usize;
                    if ids[n] == NO_ISLAND && map.labels[n] == label {
                        ids[n] = id;
                        stack.push((nx as usize, ny as usize));
                    }
                }
            }
        }
    }
    ids
}

/// Which island a contour bounds, taken from the region-side pixel flanking its
/// first directed edge. Every contour is walked with its region on the left, so
/// that pixel is always interior to the patch the contour belongs to — an outer
/// ring and the holes inside it therefore agree.
fn island_of(graph: &BoundaryGraph, map: &LabelMap, ids: &[u32], r: SegRef) -> u32 {
    let seg = &graph.segments[r.seg as usize];
    let (corner, dir) = if seg.is_ring() {
        let n = seg.points.len();
        // A ring is used forward by the region on its left, reversed by the one
        // on its right; take the first step of the chosen direction.
        let (from, to) = if r.forward {
            (seg.points[0], seg.points[1])
        } else {
            (seg.points[n - 1], seg.points[n - 2])
        };
        (from, dir_from_delta(to.x - from.x, to.y - from.y))
    } else if r.forward {
        let node = seg.start.expect("non-ring segment has a start node");
        (graph.nodes[node as usize].corner, seg.first_dir)
    } else {
        let node = seg.end.expect("non-ring segment has an end node");
        (graph.nodes[node as usize].corner, reverse(seg.last_dir))
    };

    let (px, py) = left_pixel_coord(corner.x, corner.y, dir);
    if px < 0 || py < 0 || px as u32 >= map.width || py as u32 >= map.height {
        return NO_ISLAND;
    }
    ids[py as usize * map.width as usize + px as usize]
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
    let ids = islands(map);
    // Keyed by (region, island) rather than by region alone, so disjoint patches
    // of one region become separate faces — and separate paths downstream. The
    // BTreeMap keeps face order deterministic: region ascending, then island in
    // raster-scan order.
    let mut by_island: BTreeMap<(RegionId, u32), Vec<Contour>> = BTreeMap::new();
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
            if (region as usize) < map.paints.len() {
                let island = island_of(graph, map, &ids, start);
                by_island
                    .entry((region, island))
                    .or_default()
                    .push(Contour(contour));
            }
        }
    }

    // Rings: the left side uses it forward, the right side reversed.
    for seg_id in 0..graph.segments.len() {
        let seg = &graph.segments[seg_id];
        if !seg.is_ring() {
            continue;
        }
        for (region, forward) in [(seg.left, true), (seg.right, false)] {
            if region == OUTSIDE || (region as usize) >= map.paints.len() {
                continue;
            }
            let r = SegRef {
                seg: seg_id as u32,
                forward,
            };
            let island = island_of(graph, map, &ids, r);
            by_island
                .entry((region, island))
                .or_default()
                .push(Contour(vec![r]));
        }
    }

    by_island
        .into_iter()
        .map(|((region, _island), contours)| Face { region, contours })
        .collect()
}

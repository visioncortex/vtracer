//! Stage 1: boundary-graph extraction from a [`LabelMap`].
//!
//! Pure integer arithmetic on the lattice of pixel corners `0..=W × 0..=H`.
//! Pixel `(x,y)` occupies the unit square `(x,y)..(x+1,y+1)`; boundaries run
//! along the "cracks" between differing labels.

use visioncortex::PointI32;

use super::{LabelMap, RegionId, OUTSIDE};

pub type NodeId = u32;
pub type SegId = u32;

// Unit directions, arranged clockwise in y-down screen space so that
// `(d + 1) % 4` is a right turn and `(d + 2) % 4` is a reversal.
const N: u8 = 0;
const E: u8 = 1;
const S: u8 = 2;
const W: u8 = 3;
/// (dx, dy) per direction.
const DVEC: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

#[inline]
pub(super) fn turn_right(d: u8) -> u8 {
    (d + 1) % 4
}
#[inline]
pub(super) fn straight(d: u8) -> u8 {
    d
}
#[inline]
pub(super) fn turn_left(d: u8) -> u8 {
    (d + 3) % 4
}
#[inline]
pub(super) fn reverse(d: u8) -> u8 {
    (d + 2) % 4
}

/// A directed reference to a segment: either traversed forward or reversed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegRef {
    pub seg: SegId,
    pub forward: bool,
}

/// A junction corner (degree ≥ 3) with the segment leaving it in each unit
/// direction (if any).
#[derive(Clone, Debug)]
pub struct Node {
    pub corner: PointI32,
    pub out: [Option<SegRef>; 4],
}

/// A maximal boundary chain between two nodes, or a nodeless ring.
#[derive(Clone, Debug)]
pub struct Segment {
    /// Lattice polyline; `len >= 2`. For a ring, `points[0] == points[last]`.
    pub points: Vec<PointI32>,
    pub start: Option<NodeId>,
    pub end: Option<NodeId>,
    /// Region on the left when traversing forward (y-down convention).
    pub left: RegionId,
    pub right: RegionId,
    /// Direction of the first edge (leaving `start`); unused for rings.
    pub first_dir: u8,
    /// Direction of the last edge (arriving at `end`); unused for rings.
    pub last_dir: u8,
}

impl Segment {
    pub fn is_ring(&self) -> bool {
        self.start.is_none()
    }
}

/// The extracted boundary graph. Faces are assembled separately (see `face`).
pub struct BoundaryGraph {
    pub nodes: Vec<Node>,
    pub segments: Vec<Segment>,
}

struct Extractor<'a> {
    map: &'a LabelMap,
    w: i32,
    h: i32,
    /// NodeId per lattice corner, `u32::MAX` if not a node. Size (W+1)(H+1).
    node_at: Vec<NodeId>,
    /// Visited flags for undirected unit edges.
    visited_v: Vec<bool>, // vertical edge (x in 0..=W, y in 0..H): y*(W+1)+x
    visited_h: Vec<bool>, // horizontal edge (x in 0..W, y in 0..=H): y*W + x
    nodes: Vec<Node>,
    segments: Vec<Segment>,
}

impl<'a> Extractor<'a> {
    fn new(map: &'a LabelMap) -> Self {
        let w = map.width as i32;
        let h = map.height as i32;
        let cw = (map.width + 1) as usize;
        let ch = (map.height + 1) as usize;
        Extractor {
            map,
            w,
            h,
            node_at: vec![u32::MAX; cw * ch],
            visited_v: vec![false; (map.width as usize + 1) * map.height as usize],
            visited_h: vec![false; map.width as usize * (map.height as usize + 1)],
            nodes: Vec::new(),
            segments: Vec::new(),
        }
    }

    #[inline]
    fn corner_index(&self, x: i32, y: i32) -> usize {
        y as usize * (self.w as usize + 1) + x as usize
    }

    /// 4-bit edge mask (N,E,S,W) present at corner `(x,y)`.
    fn edge_mask(&self, x: i32, y: i32) -> u8 {
        let nw = self.map.label(x - 1, y - 1);
        let ne = self.map.label(x, y - 1);
        let sw = self.map.label(x - 1, y);
        let se = self.map.label(x, y);
        let mut m = 0u8;
        if nw != ne {
            m |= 1 << N;
        }
        if ne != se {
            m |= 1 << E;
        }
        if sw != se {
            m |= 1 << S;
        }
        if nw != sw {
            m |= 1 << W;
        }
        m
    }

    /// (left, right) regions flanking the directed edge leaving `(x,y)` in `d`.
    fn side_pixels(&self, x: i32, y: i32, d: u8) -> (RegionId, RegionId) {
        let nw = self.map.label(x - 1, y - 1);
        let ne = self.map.label(x, y - 1);
        let sw = self.map.label(x - 1, y);
        let se = self.map.label(x, y);
        match d {
            N => (nw, ne),
            E => (ne, se),
            S => (se, sw),
            W => (sw, nw),
            _ => unreachable!(),
        }
    }

    /// Mark/query an undirected unit edge leaving `(x,y)` in direction `d`.
    /// Returns the canonical (is_vertical, index).
    fn edge_slot(&self, x: i32, y: i32, d: u8) -> (bool, usize) {
        match d {
            N => (true, (y - 1) as usize * (self.w as usize + 1) + x as usize),
            S => (true, y as usize * (self.w as usize + 1) + x as usize),
            E => (false, y as usize * self.w as usize + x as usize),
            W => (false, y as usize * self.w as usize + (x - 1) as usize),
            _ => unreachable!(),
        }
    }

    fn is_visited(&self, x: i32, y: i32, d: u8) -> bool {
        let (v, i) = self.edge_slot(x, y, d);
        if v {
            self.visited_v[i]
        } else {
            self.visited_h[i]
        }
    }

    fn mark_visited(&mut self, x: i32, y: i32, d: u8) {
        let (v, i) = self.edge_slot(x, y, d);
        if v {
            self.visited_v[i] = true;
        } else {
            self.visited_h[i] = true;
        }
    }

    /// Pass A — classify corners and allocate node ids for degree ≥ 3.
    fn classify(&mut self) {
        for y in 0..=self.h {
            for x in 0..=self.w {
                let deg = self.edge_mask(x, y).count_ones();
                if deg >= 3 {
                    let id = self.nodes.len() as NodeId;
                    self.nodes.push(Node {
                        corner: PointI32 { x, y },
                        out: [None; 4],
                    });
                    let ci = self.corner_index(x, y);
                    self.node_at[ci] = id;
                }
            }
        }
    }

    fn node_id(&self, x: i32, y: i32) -> Option<NodeId> {
        let id = self.node_at[self.corner_index(x, y)];
        if id == u32::MAX {
            None
        } else {
            Some(id)
        }
    }

    /// Walk from `(x0,y0)` heading `d0` until a node (or, for rings, back to
    /// the start). Returns the polyline, the final heading, and the corner
    /// walked to. Marks every traversed edge visited.
    fn walk(&mut self, x0: i32, y0: i32, d0: u8) -> (Vec<PointI32>, u8, i32, i32) {
        let mut points = vec![PointI32 { x: x0, y: y0 }];
        let (mut cx, mut cy, mut d) = (x0, y0, d0);
        loop {
            self.mark_visited(cx, cy, d);
            let (dx, dy) = DVEC[d as usize];
            let (nx, ny) = (cx + dx, cy + dy);
            points.push(PointI32 { x: nx, y: ny });

            let mask = self.edge_mask(nx, ny);
            if mask.count_ones() >= 3 {
                return (points, d, nx, ny); // reached a node
            }
            if nx == x0 && ny == y0 {
                return (points, d, nx, ny); // closed ring
            }
            // Degree-2: continue via the unique present edge that is not the
            // reverse of how we arrived.
            let rev = reverse(d);
            let mut nd = d;
            for cand in 0..4u8 {
                if cand != rev && (mask & (1 << cand)) != 0 {
                    nd = cand;
                    break;
                }
            }
            d = nd;
            cx = nx;
            cy = ny;
        }
    }

    /// Pass B — trace node-to-node segments.
    fn trace_segments(&mut self) {
        let node_corners: Vec<PointI32> = self.nodes.iter().map(|n| n.corner).collect();
        for (nid, corner) in node_corners.iter().enumerate() {
            let nid = nid as NodeId;
            let (x, y) = (corner.x, corner.y);
            let mask = self.edge_mask(x, y);
            for d in 0..4u8 {
                if (mask & (1 << d)) == 0 || self.is_visited(x, y, d) {
                    continue;
                }
                let (left, right) = self.side_pixels(x, y, d);
                let (points, last_dir, ex, ey) = self.walk(x, y, d);
                let end = self
                    .node_id(ex, ey)
                    .expect("segment must end at a node");

                let seg_id = self.segments.len() as SegId;
                self.segments.push(Segment {
                    points,
                    start: Some(nid),
                    end: Some(end),
                    left,
                    right,
                    first_dir: d,
                    last_dir,
                });
                self.nodes[nid as usize].out[d as usize] = Some(SegRef {
                    seg: seg_id,
                    forward: true,
                });
                // Leaving the end node backward along this segment.
                let back = reverse(last_dir);
                self.nodes[end as usize].out[back as usize] = Some(SegRef {
                    seg: seg_id,
                    forward: false,
                });
            }
        }
    }

    /// Pass C — closed rings from any remaining unvisited boundary edges.
    fn trace_rings(&mut self) {
        for y in 0..=self.h {
            for x in 0..=self.w {
                let mask = self.edge_mask(x, y);
                for d in 0..4u8 {
                    if (mask & (1 << d)) == 0 || self.is_visited(x, y, d) {
                        continue;
                    }
                    let (left, right) = self.side_pixels(x, y, d);
                    let (points, _last, _ex, _ey) = self.walk(x, y, d);
                    self.segments.push(Segment {
                        points,
                        start: None,
                        end: None,
                        left,
                        right,
                        first_dir: d,
                        last_dir: 0,
                    });
                }
            }
        }
    }
}

impl BoundaryGraph {
    pub fn extract(map: &LabelMap) -> BoundaryGraph {
        let mut ex = Extractor::new(map);
        ex.classify();
        ex.trace_segments();
        ex.trace_rings();
        BoundaryGraph {
            nodes: ex.nodes,
            segments: ex.segments,
        }
    }
}

/// Left region flanking the directed edge leaving `(x,y)` in `d` — used by the
/// face-assembly successor rule against a [`LabelMap`].
pub(super) fn left_pixel_at(map: &LabelMap, x: i32, y: i32, d: u8) -> RegionId {
    let nw = map.label(x - 1, y - 1);
    let ne = map.label(x, y - 1);
    let sw = map.label(x - 1, y);
    let se = map.label(x, y);
    match d {
        N => nw,
        E => ne,
        S => se,
        W => sw,
        _ => OUTSIDE,
    }
}

// Direction constants and edge-present test needed by face assembly.
pub(super) fn edge_present(map: &LabelMap, x: i32, y: i32, d: u8) -> bool {
    let nw = map.label(x - 1, y - 1);
    let ne = map.label(x, y - 1);
    let sw = map.label(x - 1, y);
    let se = map.label(x, y);
    match d {
        N => nw != ne,
        E => ne != se,
        S => sw != se,
        W => nw != sw,
        _ => false,
    }
}

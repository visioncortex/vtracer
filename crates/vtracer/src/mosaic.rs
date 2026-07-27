//! Mosaic mode: a seam-free, gapless tessellation.
//!
//! Instead of tracing every region independently (which lets neighboring
//! smoothed boundaries diverge and crack), the mosaic pipeline is topological:
//!
//! ```text
//! LabelMap → boundary graph → faces → fit each segment ONCE → compose
//! ```
//!
//! Every boundary curve exists exactly once; the two adjacent regions
//! reference the same fitted geometry, one traversed reversed. Reversal is
//! exact, so the serialized coordinates match on both sides — no seams.
//!
//! Stages 1–2 (graph + faces) are pure integer arithmetic on the lattice of
//! pixel corners. Only fitting (stage 3) is floating point.

mod compose;
mod face;
mod fit;
mod graph;

pub use compose::compose_mosaic;
pub use fit::{
    FittedSegment, PixelSegmentFitter, PolygonSegmentFitter, SegmentFitter, SplineSegmentFitter,
};
pub use graph::{BoundaryGraph, Node, Segment, SegRef};

use crate::ir::{Paint, Segmentation};

/// A dense region id. [`OUTSIDE`] marks keyed/transparent/out-of-bounds pixels.
pub type RegionId = u32;

/// Sentinel label for pixels outside any region.
pub const OUTSIDE: RegionId = u32::MAX;

/// A flat partition of the canvas: one region id per pixel, plus the paint for
/// each region. This is the sole input to the boundary-graph extractor.
#[derive(Debug, Clone)]
pub struct LabelMap {
    pub width: u32,
    pub height: u32,
    /// One label per pixel in row-major order; `OUTSIDE` for uncovered pixels.
    pub labels: Vec<RegionId>,
    /// Paint per region, indexed by label.
    pub paints: Vec<Paint>,
}

impl LabelMap {
    /// Flatten a layered [`Segmentation`] top-down into a flat partition: each
    /// pixel takes the paint of the topmost layer covering it. Layers are
    /// bottom-to-top, so painting them in order lets higher layers win.
    pub fn from_segmentation(seg: &Segmentation) -> Self {
        let w = seg.width as usize;
        let h = seg.height as usize;
        let mut labels = vec![OUTSIDE; w * h];
        let paints: Vec<Paint> = seg.layers.iter().map(|l| l.paint).collect();

        for (i, layer) in seg.layers.iter().enumerate() {
            let mask = &layer.mask;
            for ly in 0..mask.image.height {
                for lx in 0..mask.image.width {
                    if mask.image.get_pixel(lx, ly) {
                        let gx = mask.offset.x + lx as i32;
                        let gy = mask.offset.y + ly as i32;
                        if gx >= 0 && gy >= 0 && (gx as usize) < w && (gy as usize) < h {
                            labels[gy as usize * w + gx as usize] = i as RegionId;
                        }
                    }
                }
            }
        }

        LabelMap {
            width: seg.width,
            height: seg.height,
            labels,
            paints,
        }
    }

    /// Label at pixel `(x, y)`, or [`OUTSIDE`] for out-of-bounds coordinates.
    /// Treating outside as a real label removes all image-border special cases.
    #[inline]
    pub fn label(&self, x: i32, y: i32) -> RegionId {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return OUTSIDE;
        }
        self.labels[y as usize * self.width as usize + x as usize]
    }

    /// Merge neighbouring regions whose colors are within `max_diff` of each
    /// other (the metric is the clustering one: sum of per-channel absolute
    /// differences, and clustering keeps neighbours together when
    /// `diff <= deepen_diff`).
    ///
    /// The stacked hierarchy deliberately splits a gradient into layers one
    /// `deepen_diff` apart — that's what makes stacking smooth. Flattened into
    /// a mosaic, that layering degenerates into abutting faces with barely
    /// distinguishable fills. This pass undoes it: agglomerative union-find
    /// over the adjacency graph, most-similar pairs first, with each merged
    /// region's color re-derived as the area-weighted mean so chains only
    /// combine while they genuinely stay within `max_diff`.
    ///
    /// `max_diff == 0` still merges *identical*-color neighbours — a boundary
    /// between two same-colored faces is never useful. Pass a negative value
    /// to disable merging entirely.
    pub fn merge_similar(&mut self, max_diff: i32) {
        let n = self.paints.len();
        if max_diff < 0 || n < 2 {
            return;
        }

        // Area and summed color per region, for weighted mean colors.
        let mut area = vec![0u64; n];
        for &l in &self.labels {
            if l != OUTSIDE {
                area[l as usize] += 1;
            }
        }
        let mut sum: Vec<[u64; 3]> = (0..n)
            .map(|i| {
                let c = self.paints[i].color();
                [
                    c.r as u64 * area[i],
                    c.g as u64 * area[i],
                    c.b as u64 * area[i],
                ]
            })
            .collect();

        // Adjacency pairs (right/down scan covers 4-connectivity once).
        let (w, h) = (self.width as i32, self.height as i32);
        let mut pairs: Vec<(RegionId, RegionId)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for y in 0..h {
            for x in 0..w {
                let a = self.label(x, y);
                if a == OUTSIDE {
                    continue;
                }
                for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                    let b = self.label(nx, ny);
                    if b == OUTSIDE || b == a {
                        continue;
                    }
                    let key = (a.min(b), a.max(b));
                    if seen.insert(key) {
                        pairs.push(key);
                    }
                }
            }
        }

        let diff = |sa: &[u64; 3], aa: u64, sb: &[u64; 3], ab: u64| -> i32 {
            let mut d = 0i64;
            for k in 0..3 {
                d += ((sa[k] / aa.max(1)) as i64 - (sb[k] / ab.max(1)) as i64).abs();
            }
            d as i32
        };

        // Most-similar pairs first, so gradient chains coalesce around their
        // closest links; ties break on ids for determinism.
        pairs.sort_by_key(|&(a, b)| {
            (
                diff(&sum[a as usize], area[a as usize], &sum[b as usize], area[b as usize]),
                a,
                b,
            )
        });

        let mut parent: Vec<RegionId> = (0..n as RegionId).collect();
        fn find(parent: &mut [RegionId], mut i: RegionId) -> RegionId {
            while parent[i as usize] != i {
                parent[i as usize] = parent[parent[i as usize] as usize];
                i = parent[i as usize];
            }
            i
        }

        // Colors move as regions absorb one another, so re-sweep the candidate
        // pairs until nothing merges. Each union is O(α); the sweep count is
        // tiny in practice (colors only ever move toward each other's mean).
        loop {
            let mut changed = false;
            for &(a, b) in &pairs {
                let ra = find(&mut parent, a);
                let rb = find(&mut parent, b);
                if ra == rb {
                    continue;
                }
                let (ia, ib) = (ra as usize, rb as usize);
                if diff(&sum[ia], area[ia], &sum[ib], area[ib]) <= max_diff {
                    parent[ib] = ra;
                    for k in 0..3 {
                        sum[ia][k] += sum[ib][k];
                    }
                    area[ia] += area[ib];
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // Compact surviving roots into dense ids and rewrite labels + paints.
        let mut remap: Vec<RegionId> = vec![OUTSIDE; n];
        let mut paints: Vec<Paint> = Vec::new();
        for l in &mut self.labels {
            if *l == OUTSIDE {
                continue;
            }
            let root = find(&mut parent, *l);
            if remap[root as usize] == OUTSIDE {
                remap[root as usize] = paints.len() as RegionId;
                let (s, a) = (&sum[root as usize], area[root as usize].max(1));
                paints.push(Paint::Solid(visioncortex::Color::new(
                    (s[0] / a) as u8,
                    (s[1] / a) as u8,
                    (s[2] / a) as u8,
                )));
            }
            *l = remap[root as usize];
        }
        self.paints = paints;
    }
}

#[cfg(test)]
mod tests {
    use super::face::{assemble, Face};
    use super::graph::BoundaryGraph;
    use super::*;
    use crate::ir::Paint;
    use visioncortex::{Color, PointF64};

    /// Build a label map from a row-major grid (for tests).
    fn grid(width: u32, height: u32, labels: Vec<RegionId>) -> LabelMap {
        let max = labels.iter().filter(|&&l| l != OUTSIDE).copied().max();
        let n = max.map(|m| m as usize + 1).unwrap_or(0);
        let paints = (0..n).map(|_| Paint::Solid(Color::new(0, 0, 0))).collect();
        LabelMap {
            width,
            height,
            labels,
            paints,
        }
    }

    /// Reconstruct a face's contour polygons in exact lattice coordinates.
    fn face_polygons(graph: &BoundaryGraph, face: &Face) -> Vec<Vec<PointF64>> {
        face.contours
            .iter()
            .map(|contour| {
                let mut ring: Vec<PointF64> = Vec::new();
                for (i, sref) in contour.0.iter().enumerate() {
                    let pts = &graph.segments[sref.seg as usize].points;
                    let ordered: Vec<PointF64> = if sref.forward {
                        pts.iter().map(|p| PointF64 { x: p.x as f64, y: p.y as f64 }).collect()
                    } else {
                        pts.iter().rev().map(|p| PointF64 { x: p.x as f64, y: p.y as f64 }).collect()
                    };
                    if i == 0 {
                        ring.extend(ordered);
                    } else {
                        ring.extend(ordered[1..].iter().copied());
                    }
                }
                ring
            })
            .collect()
    }

    fn is_left(a: PointF64, b: PointF64, p: PointF64) -> f64 {
        (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y)
    }

    /// Winding number of point `p` w.r.t. a closed ring (last == first).
    fn winding(ring: &[PointF64], p: PointF64) -> i32 {
        let mut wn = 0;
        for w in ring.windows(2) {
            let (a, b) = (w[0], w[1]);
            if a.y <= p.y {
                if b.y > p.y && is_left(a, b, p) > 0.0 {
                    wn += 1;
                }
            } else if b.y <= p.y && is_left(a, b, p) < 0.0 {
                wn -= 1;
            }
        }
        wn
    }

    /// The strongest guarantee: rasterize the composed faces at pixel centers
    /// and assert the result is byte-identical to the input label map.
    fn assert_pixel_roundtrip(map: &LabelMap) {
        let graph = BoundaryGraph::extract(map);
        let faces = assemble(&graph, map);
        let polys: Vec<(RegionId, Vec<Vec<PointF64>>)> = faces
            .iter()
            .map(|f| (f.region, face_polygons(&graph, f)))
            .collect();

        for y in 0..map.height as i32 {
            for x in 0..map.width as i32 {
                let center = PointF64 {
                    x: x as f64 + 0.5,
                    y: y as f64 + 0.5,
                };
                let mut hits: Vec<RegionId> = Vec::new();
                for (region, rings) in &polys {
                    let wn: i32 = rings.iter().map(|r| winding(r, center)).sum();
                    if wn != 0 {
                        hits.push(*region);
                    }
                }
                let expected = map.label(x, y);
                if expected == OUTSIDE {
                    assert!(hits.is_empty(), "({x},{y}) OUTSIDE but covered by {hits:?}");
                } else {
                    assert_eq!(
                        hits,
                        vec![expected],
                        "({x},{y}) expected region {expected}, got {hits:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn single_region_is_one_ring() {
        let map = grid(3, 2, vec![0; 6]);
        let graph = BoundaryGraph::extract(&map);
        assert_eq!(graph.nodes.len(), 0, "no junctions in a single region");
        assert_eq!(graph.segments.len(), 1, "one border ring");
        assert!(graph.segments[0].is_ring());
        assert_pixel_roundtrip(&map);
    }

    #[test]
    fn vertical_split() {
        // 4x2, left half 0, right half 1.
        let map = grid(4, 2, vec![0, 0, 1, 1, 0, 0, 1, 1]);
        let graph = BoundaryGraph::extract(&map);
        // Two border junctions where the split meets the top and bottom edges.
        assert_eq!(graph.nodes.len(), 2);
        assert_pixel_roundtrip(&map);
    }

    #[test]
    fn t_junction() {
        // top row one region, bottom row split — a degree-3 interior node.
        let map = grid(2, 2, vec![0, 0, 1, 2]);
        assert_pixel_roundtrip(&map);
    }

    #[test]
    fn checkerboard_pinch() {
        // A B / B A — the center corner is a degree-4 pinch; each region is two
        // lobes touching there. (The four boundary/border corners are degree-3
        // nodes too, per the border rule — so 5 nodes total.) The round-trip is
        // the real check that the pinch produces exact, simple contours.
        let map = grid(2, 2, vec![0, 1, 1, 0]);
        let graph = BoundaryGraph::extract(&map);
        let has_degree4 = graph.nodes.iter().any(|n| {
            let c = n.corner;
            n.out.iter().filter(|o| o.is_some()).count() == 4 && c.x == 1 && c.y == 1
        });
        assert!(has_degree4, "expected a degree-4 pinch node at the center");
        assert_pixel_roundtrip(&map);
    }

    #[test]
    fn disjoint_patches_of_one_region_get_separate_faces() {
        // Region 0 appears as two islands, separated by a column of region 1.
        // Each island must get its own face, so they cannot share a path.
        #[rustfmt::skip]
        let map = grid(3, 2, vec![
            0, 1, 0,
            0, 1, 0,
        ]);
        let graph = BoundaryGraph::extract(&map);
        let faces = assemble(&graph, &map);

        assert_eq!(
            faces.iter().filter(|f| f.region == 0).count(),
            2,
            "each island of region 0 gets its own face"
        );
        assert_eq!(faces.len(), 3, "two islands of region 0, plus region 1");
        assert_pixel_roundtrip(&map);
    }

    #[test]
    fn diagonal_lobes_share_one_face() {
        // A B / B A — region 0's lobes meet only at the center corner, which the
        // successor rule pinches into a single contour. They must stay in one
        // face: splitting them could separate a hole contour from the ring that
        // encloses it, and a lone hole ring fills solid under `nonzero`.
        #[rustfmt::skip]
        let map = grid(2, 2, vec![
            0, 1,
            1, 0,
        ]);
        let graph = BoundaryGraph::extract(&map);
        let faces = assemble(&graph, &map);

        assert_eq!(
            faces.iter().filter(|f| f.region == 0).count(),
            1,
            "diagonally touching lobes stay in one face"
        );
        assert_pixel_roundtrip(&map);
    }

    #[test]
    fn nested_rings() {
        // Concentric squares: 0 outer, 1 middle, 2 center.
        let l = |x: i32, y: i32| -> RegionId {
            let d = x.min(y).min(5 - x).min(5 - y);
            match d {
                0 => 0,
                1 => 1,
                _ => 2,
            }
        };
        let mut labels = Vec::new();
        for y in 0..6 {
            for x in 0..6 {
                labels.push(l(x, y));
            }
        }
        assert_pixel_roundtrip(&grid(6, 6, labels));
    }

    #[test]
    fn outside_region_border_touching() {
        // A region that does not fill the canvas; the rest is OUTSIDE.
        let mut labels = vec![OUTSIDE; 16];
        for y in 1..3 {
            for x in 1..3 {
                labels[y * 4 + x] = 0;
            }
        }
        assert_pixel_roundtrip(&grid(4, 4, labels));
    }

    #[test]
    fn spline_segments_pin_endpoints_to_lattice() {
        use super::fit::{FittedGeom, SegmentFitter, SplineSegmentFitter};
        // A shape with junctions so there are open (non-ring) segments.
        let map = grid(4, 4, vec![
            0, 0, 1, 1,
            0, 0, 1, 1,
            2, 2, 1, 1,
            2, 2, 2, 2,
        ]);
        let graph = BoundaryGraph::extract(&map);
        let fitter = SplineSegmentFitter::default();
        let mut checked = 0;
        for seg in &graph.segments {
            if seg.is_ring() {
                continue;
            }
            let fitted = fitter.fit_open(seg);
            let start = PointF64 { x: seg.points[0].x as f64, y: seg.points[0].y as f64 };
            let end = {
                let p = seg.points[seg.points.len() - 1];
                PointF64 { x: p.x as f64, y: p.y as f64 }
            };
            match fitted.geom {
                FittedGeom::Beziers(b) => {
                    assert_eq!(b.first().unwrap()[0], start, "start pinned to node");
                    assert_eq!(b.last().unwrap()[3], end, "end pinned to node");
                }
                FittedGeom::Polyline(p) => {
                    assert_eq!(*p.first().unwrap(), start);
                    assert_eq!(*p.last().unwrap(), end);
                }
            }
            checked += 1;
        }
        assert!(checked > 0, "expected some open segments");
    }

    /// Build a label map with explicit per-region gray levels.
    fn gray_grid(width: u32, height: u32, labels: Vec<RegionId>, grays: &[u8]) -> LabelMap {
        LabelMap {
            width,
            height,
            labels,
            paints: grays
                .iter()
                .map(|&g| Paint::Solid(Color::new(g, g, g)))
                .collect(),
        }
    }

    #[test]
    fn merge_similar_rejoins_close_neighbours() {
        // Three vertical strips: 100 | 106 | 220. Diff(0,1) = 18 ≤ 20 → merge;
        // the merged mean (103) vs 220 stays far apart.
        #[rustfmt::skip]
        let mut map = gray_grid(3, 2, vec![
            0, 1, 2,
            0, 1, 2,
        ], &[100, 106, 220]);
        map.merge_similar(20);

        assert_eq!(map.paints.len(), 2, "strips 0 and 1 merge; 2 survives");
        assert_eq!(map.label(0, 0), map.label(1, 0));
        assert_ne!(map.label(0, 0), map.label(2, 0));
        // Area-weighted mean of two equal strips of 100 and 106.
        assert_eq!(map.paints[map.label(0, 0) as usize].color().r, 103);
        assert_pixel_roundtrip(&map);
    }

    #[test]
    fn merge_similar_uses_running_means_not_original_colors() {
        // Gradient chain 100 | 103 | 106 with threshold 9 (grays g apart diff
        // by 3g across the three channels). The closest pair merges first
        // (ties broken by id → strips 0,1 → mean 101); the merged region vs
        // 106 is then 15 apart, over threshold — the chain must NOT collapse
        // transitively into one region on the strength of the original colors.
        #[rustfmt::skip]
        let mut map = gray_grid(3, 1, vec![0, 1, 2], &[100, 103, 106]);
        map.merge_similar(9);

        assert_eq!(map.paints.len(), 2, "running mean stops the chain");
        assert_eq!(map.label(0, 0), map.label(1, 0));
        assert_ne!(map.label(1, 0), map.label(2, 0));
    }

    #[test]
    fn merge_similar_ignores_outside_and_non_neighbours() {
        // Two same-colored regions separated by OUTSIDE: not adjacent, so they
        // must stay distinct faces (merging them would create a disjoint
        // region, which face assembly handles, but the ids must stay honest to
        // the partition).
        #[rustfmt::skip]
        let mut map = gray_grid(3, 1, vec![0, OUTSIDE, 1], &[100, 100]);
        map.merge_similar(20);

        assert_eq!(map.paints.len(), 2, "non-adjacent regions never merge");
        assert_eq!(map.label(1, 0), OUTSIDE, "outside pixels are untouched");
        assert_pixel_roundtrip(&map);
    }

    #[test]
    fn merge_similar_zero_threshold_merges_only_identical_colors() {
        // Regions 0 and 1 share a color; region 2 differs by one level. At
        // threshold 0 the identical pair merges, the near-identical one stays.
        #[rustfmt::skip]
        let mut map = gray_grid(3, 1, vec![0, 1, 2], &[100, 100, 101]);
        map.merge_similar(0);
        assert_eq!(map.paints.len(), 2, "identical neighbours merge at 0");
        assert_eq!(map.label(0, 0), map.label(1, 0));
        assert_ne!(map.label(1, 0), map.label(2, 0));

        // A negative threshold disables merging entirely.
        let labels = vec![0, 1, 0, 1];
        let mut map = gray_grid(2, 2, labels.clone(), &[100, 100]);
        map.merge_similar(-1);
        assert_eq!(map.labels, labels);
        assert_eq!(map.paints.len(), 2);
    }

    #[test]
    fn compose_mosaic_merges_gradient_faces() {
        use super::compose_mosaic;
        use super::fit::PixelSegmentFitter;
        use crate::ir::{Layer, RegionMask, Segmentation};
        use visioncortex::BinaryImage;

        // A 6x2 canvas of three 2px strips, one gradient step apart (diff 6),
        // as bottom-to-top layers — exactly what a stacked gradient flattens
        // into. With merging they are one face; without, three.
        let mut seg = Segmentation::new(6, 2);
        for (i, g) in [(0, 100u8), (1, 102), (2, 104)] {
            let mut image = BinaryImage::new_w_h(2, 2);
            for y in 0..2 {
                for x in 0..2 {
                    image.set_pixel(x, y, true);
                }
            }
            seg.layers.push(Layer {
                paint: Paint::Solid(Color::new(g, g, g)),
                mask: RegionMask::new(
                    image,
                    visioncortex::PointI32 { x: i * 2, y: 0 },
                ),
            });
        }

        let unmerged = compose_mosaic(&seg, &PixelSegmentFitter, 0);
        let merged = compose_mosaic(&seg, &PixelSegmentFitter, 16);
        assert_eq!(unmerged.shapes.len(), 3);
        assert_eq!(merged.shapes.len(), 1, "gradient strips coalesce into one face");
        assert_eq!(merged.shapes[0].paint.color().r, 102, "area-weighted mean");
    }

    #[test]
    fn random_maps_roundtrip() {
        // Deterministic LCG; connectivity not required.
        let mut state: u64 = 0x1234_5678_9abc_def0;
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 33) as u32
        };
        for _ in 0..40 {
            let w = 2 + next() % 10;
            let h = 2 + next() % 10;
            let nlabels = 1 + next() % 5;
            let labels: Vec<RegionId> = (0..w * h).map(|_| next() % nlabels).collect();
            assert_pixel_roundtrip(&grid(w, h, labels));
        }
    }
}

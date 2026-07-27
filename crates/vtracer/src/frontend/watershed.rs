//! Hierarchical watershed frontend — region forming on the pixel graph.
//!
//! The image is treated as a 4-adjacency edge-weighted graph (edge weight =
//! color difference between the two pixels; no gradient image is built). On it
//! we compute the watershed hierarchy by **volume extinction**, following:
//!
//! * Cousty, Bertrand, Najman, Couprie, *Watershed Cuts: Minimum Spanning
//!   Forests and the Drop of Water Principle*, IEEE TPAMI 31(8), 2009.
//! * Najman, Cousty, Perret, *Playing with Kruskal: Algorithms for
//!   Morphological Trees in Edge-Weighted Graphs*, ISMM 2013.
//!
//! The work is split in two so the expensive part can be cached (see
//! [`crate::Session`]):
//!
//! * [`WatershedHierarchy::build`] — Kruskal over counting-sorted edges builds
//!   the binary partition tree (a flat `parents` array, leaves `0..n`,
//!   internal nodes created in altitude order); a leaves-to-root pass computes
//!   each subtree's area and volume; each internal node's *persistence* (the
//!   volume of the smaller of the two merged basins) becomes the saliency of
//!   its MST edge. This depends only on the image — no tuning parameters.
//! * [`WatershedHierarchy::cut`] — cutting at level λ is single-linkage over
//!   MST edges with persistence ≤ λ (every pixel gets a label, no
//!   watershed-line pixels), small basins are absorbed, and the surviving
//!   merge tree above λ becomes the output layer stack.
//!
//! The cut emits a **stacked hierarchy**, the same principle as the color
//! clustering frontend: the root (whole canvas, mean color) is painted first,
//! then progressively finer ancestor regions, then the final regions on top.
//! Sub-pixel gaps between abutting regions therefore show their common
//! ancestor's color instead of an unrelated backdrop, and stacked mode stays
//! seam-free by overdraw. Flattening top-down (what cutout does) recovers the
//! exact partition, because the final regions are painted last.
//!
//! Everything is integer and allocation-flat: counting sort over 256 weight
//! buckets, path-halving union-find, `u32` node ids. Deterministic across
//! platforms.

use visioncortex::{BinaryImage, Color, ColorImage, PointI32};

use crate::error::Error;
use crate::ir::{Layer, Paint, RegionMask, Segmentation};

use super::Frontend;

/// Cap on the total painted area of ancestor layers, as a multiple of the
/// canvas: keeps a pathological hierarchy (long chains of near-equal
/// persistence) from ballooning the stacked output. The root and the final
/// regions are always emitted, so coverage never depends on this.
const ANCESTOR_AREA_BUDGET: usize = 3;

/// Watershed frontend: hierarchical watershed by volume, cut at `detail`.
#[derive(Debug, Clone)]
pub struct WatershedFrontend {
    /// Detail level (0..=255): where to cut the hierarchy. Each +25.5 roughly
    /// doubles the region count; 0 collapses the image to a single region.
    pub detail: u8,
    /// Absorb regions smaller than this many pixels into their most
    /// color-similar neighbour after the cut (0 = keep all).
    pub min_area: usize,
}

impl Default for WatershedFrontend {
    fn default() -> Self {
        Self {
            detail: 128,
            min_area: 16,
        }
    }
}

/// Flat union-find over `u32` ids with path halving.
struct Uf(Vec<u32>);

impl Uf {
    fn new(n: usize) -> Self {
        Uf((0..n as u32).collect())
    }

    fn find(&mut self, mut x: u32) -> u32 {
        while self.0[x as usize] != x {
            self.0[x as usize] = self.0[self.0[x as usize] as usize];
            x = self.0[x as usize];
        }
        x
    }

    /// Union by attaching `b`'s root under `a`'s. Caller passes roots.
    fn link(&mut self, a: u32, b: u32) {
        self.0[b as usize] = a;
    }
}

/// Edge weight: max per-channel absolute difference (L∞), the same family of
/// channel-difference metric the rest of vtracer uses. 0..=255.
#[inline]
fn edge_weight(a: Color, b: Color) -> u8 {
    let dr = a.r.abs_diff(b.r);
    let dg = a.g.abs_diff(b.g);
    let db = a.b.abs_diff(b.b);
    dr.max(dg).max(db)
}

/// The image's watershed hierarchy: the minimum spanning tree of the pixel
/// graph with a persistence (volume extinction) per edge. Building it is the
/// expensive step and depends only on the image; [`cut`](Self::cut) derives a
/// [`Segmentation`] for any detail level in near-linear time, so interactive
/// re-tuning never repays the build (see [`crate::Session`]).
pub struct WatershedHierarchy {
    width: usize,
    height: usize,
    /// MST edges as pixel pairs, in Kruskal creation order.
    mst: Vec<(u32, u32)>,
    /// Persistence (volume of the smaller merged basin) per MST edge.
    pers: Vec<u64>,
    /// MST edge indices by ascending (persistence, index) — the cut order.
    order: Vec<u32>,
}

impl WatershedHierarchy {
    /// Build the hierarchy: counting-sorted Kruskal → binary partition tree →
    /// volume persistence per MST edge. O(n α(n)).
    pub fn build(img: &ColorImage) -> Result<Self, Error> {
        let w = img.width;
        let h = img.height;
        if w == 0 || h == 0 {
            return Err(Error::EmptyImage);
        }
        let n = w * h;
        if n == 1 {
            return Ok(Self {
                width: w,
                height: h,
                mst: Vec::new(),
                pers: Vec::new(),
                order: Vec::new(),
            });
        }

        // --- 4-adjacency edges, counting-sorted by weight -------------------
        // Edge id encodes (pixel, direction): 2*p = right, 2*p+1 = down.
        // The per-bucket fill preserves edge-id order, so the sort is stable
        // and the whole construction is deterministic.
        let px = |i: usize| img.get_pixel(i % w, i / w);
        let mut counts = [0u32; 256];
        let mut weight_of = vec![0u8; 2 * n];
        for i in 0..n {
            let c = px(i);
            if i % w + 1 < w {
                let wgt = edge_weight(c, px(i + 1));
                weight_of[2 * i] = wgt;
                counts[wgt as usize] += 1;
            }
            if i / w + 1 < h {
                let wgt = edge_weight(c, px(i + w));
                weight_of[2 * i + 1] = wgt;
                counts[wgt as usize] += 1;
            }
        }
        let n_edges = counts.iter().map(|&c| c as usize).sum::<usize>();
        let mut start = [0usize; 256];
        let mut acc = 0usize;
        for b in 0..256 {
            start[b] = acc;
            acc += counts[b] as usize;
        }
        let mut sorted = vec![0u32; n_edges];
        let mut fill = start;
        for i in 0..n {
            if i % w + 1 < w {
                let e = 2 * i;
                let b = weight_of[e] as usize;
                sorted[fill[b]] = e as u32;
                fill[b] += 1;
            }
            if i / w + 1 < h {
                let e = 2 * i + 1;
                let b = weight_of[e] as usize;
                sorted[fill[b]] = e as u32;
                fill[b] += 1;
            }
        }

        // --- Kruskal → binary partition tree by altitude --------------------
        // Leaves 0..n are pixels; each accepted MST edge creates internal node
        // n+k whose two children are the merged components' current roots.
        // The grid is connected, so exactly n-1 internal nodes are created and
        // parent indices are always greater than child indices.
        let n_nodes = 2 * n - 1;
        let mut parent = vec![u32::MAX; n_nodes];
        let mut alt = vec![0u8; n_nodes]; // altitude; leaves at 0
        let mut child = vec![[0u32; 2]; n - 1]; // children of internal node k
        let mut mst = vec![(0u32, 0u32); n - 1]; // pixel pair of edge k
        let mut uf = Uf::new(n);
        // Current tree node representing each union-find root's component.
        let mut comp_node: Vec<u32> = (0..n as u32).collect();
        let mut next = n as u32;
        for &e in &sorted {
            let p = (e / 2) as usize;
            let q = if e % 2 == 0 { p + 1 } else { p + w };
            let (rp, rq) = (uf.find(p as u32), uf.find(q as u32));
            if rp == rq {
                continue;
            }
            let k = (next - n as u32) as usize;
            alt[next as usize] = weight_of[e as usize];
            child[k] = [comp_node[rp as usize], comp_node[rq as usize]];
            mst[k] = (p as u32, q as u32);
            parent[comp_node[rp as usize] as usize] = next;
            parent[comp_node[rq as usize] as usize] = next;
            uf.link(rp, rq);
            comp_node[rp as usize] = next;
            next += 1;
        }
        debug_assert_eq!(next as usize, n_nodes);

        // --- Volume attribute, leaves → root --------------------------------
        // area = pixels in the subtree; volume = ∫ area over altitude, i.e.
        // each node contributes area × (parent altitude − own altitude).
        // Ascending index order visits all children before their parent.
        let root = n_nodes - 1;
        let mut area = vec![0u64; n_nodes];
        for a in area.iter_mut().take(n) {
            *a = 1;
        }
        let mut volume = vec![0u64; n_nodes];
        for i in 0..root {
            let pa = parent[i] as usize;
            area[pa] += area[i];
            let rise = (alt[pa] - alt[i]) as u64; // parent is never lower
            volume[i] += area[i] * rise;
            volume[pa] += volume[i];
        }

        // --- Persistence per MST edge ----------------------------------------
        // Plateau fix first (Playing with Kruskal): equal-weight edge chains
        // create internal nodes at the same altitude as their parent; their
        // volume is not a real basin measure, so replace it with the max over
        // children while the altitude is unchanged.
        let mut corrected = volume;
        for i in n..n_nodes {
            let k = i - n;
            if i != root && alt[i] == alt[parent[i] as usize] {
                let [c0, c1] = child[k];
                corrected[i] = corrected[c0 as usize].max(corrected[c1 as usize]);
            }
        }
        // Persistence of a merge = the volume of the smaller side: the level
        // at which that basin stops existing on its own.
        let mut pers = vec![0u64; n - 1];
        for k in 0..n - 1 {
            let [c0, c1] = child[k];
            pers[k] = corrected[c0 as usize].min(corrected[c1 as usize]);
        }

        let mut order: Vec<u32> = (0..(n - 1) as u32).collect();
        order.sort_by_key(|&k| (pers[k as usize], k));

        Ok(Self {
            width: w,
            height: h,
            mst,
            pers,
            order,
        })
    }

    /// Cut the hierarchy at `detail` and emit the stacked [`Segmentation`].
    /// Near-linear; safe to call repeatedly with different parameters.
    pub fn cut(&self, img: &ColorImage, detail: u8, min_area: usize) -> Segmentation {
        let (w, h) = (self.width, self.height);
        let n = w * h;
        let m = self.mst.len();

        // --- Region formation: merge every MST edge with persistence ≤ λ ----
        // Merging leaves exactly 1 + #{edges above λ} regions, so choosing λ
        // as the k-th largest persistence targets k regions directly (ties
        // merge a little more). The persistence distribution is extremely
        // skewed — most merges are trivia at ≈ 0 — so the dial maps to a
        // region *count*, exponentially: every +25.5 of detail doubles the
        // target, from 1 region at 0 up to 1024 at 255.
        let mut uf = Uf::new(n);
        let mut split_from = 0usize;
        if m > 0 {
            let target = (2f64).powf(detail as f64 / 25.5).round() as usize;
            let target = target.clamp(1, m);
            let lambda = self.pers[self.order[m - target] as usize];
            for (i, &k) in self.order.iter().enumerate() {
                if self.pers[k as usize] > lambda {
                    break;
                }
                let (p, q) = self.mst[k as usize];
                let (rp, rq) = (uf.find(p), uf.find(q));
                if rp != rq {
                    uf.link(rp, rq);
                }
                split_from = i + 1;
            }
        }

        // --- Compact to region ids, region stats, boundary adjacency --------
        // One find per pixel; everything after this works on the (small)
        // region graph so re-cuts stay cheap.
        let mut pre_of_root = vec![u32::MAX; n];
        let mut pre = vec![0u32; n];
        let mut kp = 0usize;
        for i in 0..n {
            let r = uf.find(i as u32) as usize;
            if pre_of_root[r] == u32::MAX {
                pre_of_root[r] = kp as u32;
                kp += 1;
            }
            pre[i] = pre_of_root[r];
        }
        let mut area = vec![0u64; kp];
        let mut sum = vec![[0u64; 3]; kp];
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        for i in 0..n {
            let a = pre[i];
            let c = img.get_pixel(i % w, i / w);
            area[a as usize] += 1;
            sum[a as usize][0] += c.r as u64;
            sum[a as usize][1] += c.g as u64;
            sum[a as usize][2] += c.b as u64;
            if i % w + 1 < w && pre[i + 1] != a {
                pairs.push((a, pre[i + 1]));
            }
            if i / w + 1 < h && pre[i + w] != a {
                pairs.push((a, pre[i + w]));
            }
        }

        // --- Small-basin absorption on the region graph ----------------------
        let mut uf_r = Uf::new(kp);
        absorb_small(min_area, &pairs, &mut uf_r, &mut area, &mut sum);

        // --- Final leaf ids in raster order of first appearance --------------
        let mut leaf_of = vec![u32::MAX; kp];
        let mut leaf_root: Vec<u32> = Vec::new(); // leaf id -> absorb root
        let mut ids = vec![0u32; n];
        for i in 0..n {
            let r = uf_r.find(pre[i]) as usize;
            if leaf_of[r] == u32::MAX {
                leaf_of[r] = leaf_root.len() as u32;
                leaf_root.push(r as u32);
            }
            ids[i] = leaf_of[r];
        }
        let k = leaf_root.len();

        let mean = |s: &[u64; 3], a: u64| {
            Color::new((s[0] / a) as u8, (s[1] / a) as u8, (s[2] / a) as u8)
        };

        let mut seg = Segmentation::new(w as u32, h as u32);
        if k == 1 {
            // Single region: one solid full-canvas layer.
            let r = leaf_root[0] as usize;
            seg.layers.push(Layer {
                paint: Paint::Solid(mean(&sum[r], area[r])),
                mask: full_canvas(w, h),
            });
            return seg;
        }

        // --- Merge tree above the cut ----------------------------------------
        // Re-run the remaining merges (ascending persistence) over the final
        // regions: each one that still joins two components is a kept split.
        // Nodes 0..k are the final regions; internal nodes are created in
        // ascending persistence order, so the reverse is a root-first order in
        // which every ancestor precedes its descendants.
        let n_tree = 2 * k - 1;
        let mut tree_child: Vec<[u32; 2]> = Vec::with_capacity(k - 1);
        let mut tree_area = vec![0u64; n_tree];
        let mut tree_sum = vec![[0u64; 3]; n_tree];
        for (t, &r) in leaf_root.iter().enumerate() {
            tree_area[t] = area[r as usize];
            tree_sum[t] = sum[r as usize];
        }
        let mut uf2 = Uf::new(k);
        let mut node_rep: Vec<u32> = (0..k as u32).collect();
        let mut next = k as u32;
        for &e in &self.order[split_from..] {
            let (p, q) = self.mst[e as usize];
            let (lp, lq) = (ids[p as usize], ids[q as usize]);
            let (a, b) = (uf2.find(lp), uf2.find(lq));
            if a == b {
                continue; // rejoined by absorption; not a split anymore
            }
            let node = next as usize;
            tree_child.push([node_rep[a as usize], node_rep[b as usize]]);
            for ch in [node_rep[a as usize], node_rep[b as usize]] {
                tree_area[node] += tree_area[ch as usize];
                for c in 0..3 {
                    tree_sum[node][c] += tree_sum[ch as usize][c];
                }
            }
            uf2.link(a, b);
            node_rep[a as usize] = next;
            next += 1;
        }
        debug_assert_eq!(next as usize, n_tree);

        // Per-leaf pixel lists, for painting ancestor masks.
        let mut leaf_len = vec![0u32; k];
        for &id in &ids {
            leaf_len[id as usize] += 1;
        }
        let mut leaf_start = vec![0usize; k + 1];
        for t in 0..k {
            leaf_start[t + 1] = leaf_start[t] + leaf_len[t] as usize;
        }
        let mut leaf_px = vec![0u32; n];
        let mut fill = leaf_start.clone();
        for (i, &id) in ids.iter().enumerate() {
            leaf_px[fill[id as usize]] = i as u32;
            fill[id as usize] += 1;
        }

        // --- Emit: root, ancestors (budgeted), then the final regions --------
        let root = n_tree - 1;
        seg.layers.push(Layer {
            paint: Paint::Solid(mean(&tree_sum[root], tree_area[root])),
            mask: full_canvas(w, h),
        });
        let mut budget = ANCESTOR_AREA_BUDGET * n;
        for node in (k..root).rev() {
            let node_area = tree_area[node] as usize;
            if node_area > budget {
                continue;
            }
            budget -= node_area;
            seg.layers.push(Layer {
                paint: Paint::Solid(mean(&tree_sum[node], tree_area[node])),
                mask: node_mask(node, k, &tree_child, &leaf_start, &leaf_px, w),
            });
        }
        for t in 0..k {
            seg.layers.push(Layer {
                paint: Paint::Solid(mean(&tree_sum[t], tree_area[t])),
                mask: node_mask(t, k, &tree_child, &leaf_start, &leaf_px, w),
            });
        }
        seg
    }
}

fn full_canvas(w: usize, h: usize) -> RegionMask {
    let mut image = BinaryImage::new_w_h(w, h);
    for y in 0..h {
        for x in 0..w {
            image.set_pixel(x, y, true);
        }
    }
    RegionMask::new(image, PointI32 { x: 0, y: 0 })
}

/// Paint a tree node's region (the union of the final regions beneath it)
/// into a bbox-cropped mask.
fn node_mask(
    node: usize,
    k: usize,
    tree_child: &[[u32; 2]],
    leaf_start: &[usize],
    leaf_px: &[u32],
    w: usize,
) -> RegionMask {
    // Collect the node's leaves.
    let mut leaves: Vec<usize> = Vec::new();
    let mut stack = vec![node];
    while let Some(t) = stack.pop() {
        if t < k {
            leaves.push(t);
        } else {
            let [a, b] = tree_child[t - k];
            stack.push(a as usize);
            stack.push(b as usize);
        }
    }
    // Bounding box over all member pixels.
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for &t in &leaves {
        for &p in &leaf_px[leaf_start[t]..leaf_start[t + 1]] {
            let (x, y) = ((p as usize % w) as i32, (p as usize / w) as i32);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    let (bw, bh) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
    let mut image = BinaryImage::new_w_h(bw, bh);
    for &t in &leaves {
        for &p in &leaf_px[leaf_start[t]..leaf_start[t + 1]] {
            let (x, y) = (p as usize % w, p as usize / w);
            image.set_pixel(x - x0 as usize, y - y0 as usize, true);
        }
    }
    RegionMask::new(image, PointI32 { x: x0, y: y0 })
}

/// Absorb regions smaller than `min_area` into their most color-similar
/// neighbour, working entirely on the region graph: `pairs` are the boundary
/// adjacencies (duplicates fine), `uf` is a region-level union-find, and the
/// stats are merged along so downstream consumers see the final regions.
/// Sweeps until nothing undersized remains (or an undersized region has no
/// neighbour at all).
fn absorb_small(
    min_area: usize,
    pairs: &[(u32, u32)],
    uf: &mut Uf,
    area: &mut [u64],
    sum: &mut [[u64; 3]],
) {
    if min_area <= 1 {
        return;
    }
    let k = area.len();
    let mean_diff = |sa: &[u64; 3], aa: u64, sb: &[u64; 3], ab: u64| -> u64 {
        let mut d = 0i64;
        for ch in 0..3 {
            d += ((sa[ch] / aa) as i64 - (sb[ch] / ab) as i64).abs();
        }
        d as u64
    };
    loop {
        // best[r] = (diff, neighbour_root) for undersized root r
        let mut best: Vec<(u64, u32)> = vec![(u64::MAX, u32::MAX); k];
        let mut any_small = false;
        for &(p, q) in pairs {
            let (a, b) = (uf.find(p), uf.find(q));
            if a == b {
                continue;
            }
            for (s, t) in [(a, b), (b, a)] {
                let (su, tu) = (s as usize, t as usize);
                if area[su] < min_area as u64 {
                    any_small = true;
                    let d = mean_diff(&sum[su], area[su], &sum[tu], area[tu]);
                    if d < best[su].0 || (d == best[su].0 && t < best[su].1) {
                        best[su] = (d, t);
                    }
                }
            }
        }
        if !any_small {
            break;
        }
        let mut merged = false;
        for r in 0..k {
            let (_, tgt) = best[r];
            if tgt == u32::MAX {
                continue;
            }
            let rr = uf.find(r as u32);
            if rr as usize != r {
                continue; // already absorbed this sweep
            }
            let rt = uf.find(tgt);
            if rt == rr {
                continue;
            }
            uf.link(rt, rr);
            area[rt as usize] += area[r];
            for ch in 0..3 {
                sum[rt as usize][ch] += sum[r][ch];
            }
            merged = true;
        }
        if !merged {
            break; // isolated undersized region (e.g. whole-canvas)
        }
    }
}

impl Frontend for WatershedFrontend {
    fn segment(&self, img: &ColorImage) -> Result<Segmentation, Error> {
        Ok(WatershedHierarchy::build(img)?.cut(img, self.detail, self.min_area))
    }
}

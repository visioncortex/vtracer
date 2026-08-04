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
//!   watershed-line pixels), antialiased boundary pixels are snapped to the
//!   color-midpoint iso-line (see [`snap_boundaries`]), small basins are
//!   absorbed, and the surviving merge tree above λ becomes the output layer
//!   stack.
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
    /// Detail level: where to cut the hierarchy. The normal range is 0..=255 —
    /// each +25.5 roughly doubles the region count, 0 collapses the image to a
    /// single region. Values above 255 are practically uncapped.
    pub detail: u32,
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
    pub fn cut(&self, img: &ColorImage, detail: u32, min_area: usize) -> Segmentation {
        let (w, h) = (self.width, self.height);
        let n = w * h;
        let m = self.mst.len();

        // --- Region formation: merge every MST edge with persistence ≤ λ ----
        // Merging leaves exactly 1 + #{edges above λ} regions, so choosing λ
        // as the k-th largest persistence targets k regions directly (ties
        // merge a little more). The persistence distribution is extremely
        // skewed — most merges are trivia at ≈ 0 — so the dial maps to a
        // region *count*, exponentially: every +25.5 of detail doubles the
        // target, from 1 region at 0. The target saturates at the edge count,
        // so a large detail (≥ 25.5·log2(pixels), e.g. ≥ 612 for a 4096²
        // image) is practically uncapped: λ = min persistence, keeping every
        // basin above the zero-persistence trivia.
        let mut uf = Uf::new(n);
        if m > 0 {
            let target = (2f64).powf(detail as f64 / 25.5).round() as usize;
            let target = target.clamp(1, m);
            let lambda = self.pers[self.order[m - target] as usize];
            for &k in &self.order {
                if self.pers[k as usize] > lambda {
                    break;
                }
                let (p, q) = self.mst[k as usize];
                let (rp, rq) = (uf.find(p), uf.find(q));
                if rp != rq {
                    uf.link(rp, rq);
                }
            }
        }

        // --- Compact to region ids and region stats --------------------------
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
        for i in 0..n {
            let a = pre[i] as usize;
            let c = img.get_pixel(i % w, i / w);
            area[a] += 1;
            sum[a][0] += c.r as u64;
            sum[a][1] += c.g as u64;
            sum[a][2] += c.b as u64;
        }

        // --- Boundary snap, then boundary adjacency ---------------------------
        snap_boundaries(img, w, h, &mut pre, &mut area, &mut sum);
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        for i in 0..n {
            let a = pre[i];
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
        // Re-run all merges (ascending persistence) over the final regions:
        // each one that still joins two components is a kept split. Nodes 0..k
        // are the final regions; internal nodes are created in ascending
        // persistence order, so the reverse is a root-first order in which
        // every ancestor precedes its descendants. Below-cut edges are almost
        // all no-ops (their endpoints share a region), but not quite: boundary
        // snapping can leave a region's only adjacency running through a
        // below-cut edge, and skipping those would leave the tree unconnected.
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
        for &e in &self.order {
            let (p, q) = self.mst[e as usize];
            let (lp, lq) = (ids[p as usize], ids[q as usize]);
            if lp == lq {
                continue; // same region — the bulk of the below-cut edges
            }
            let (a, b) = (uf2.find(lp), uf2.find(lq));
            if a == b {
                continue; // already merged, or rejoined by absorption
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

/// How many 1-px boundary-snap sweeps to run: bounds the boundary movement to
/// the width of an antialiasing ramp / JPEG halo (compression ringing spreads
/// a hard edge over up to ~3 px; a plain AA ramp over 1–2 px).
const SNAP_SWEEPS: usize = 4;

/// Tolerance for the mixture test below: an antialiased blend of two region
/// colors satisfies `d(p,A) + d(p,B) = d(A,B)` exactly (L1, per-channel
/// between-ness); this slack admits sensor/JPEG noise of a few units per
/// channel without admitting genuine third colors.
const SNAP_SLACK: i32 = 16;

/// Re-assign boundary pixels to whichever adjacent region's mean color is
/// closest (strictly closer than their own region's mean, L1).
///
/// The minimum-spanning-forest cut routes the boundary through whichever
/// crack of an antialiasing ramp has the minutely-largest weight, so along a
/// smooth edge it meanders ±1–2 px with the pixel noise and the fitted curves
/// visibly wave (crisp synthetic edges are unaffected: their boundary pixels
/// sit exactly at a region's mean). Snapping by color lands the boundary on
/// the color-midpoint iso-line of the ramp instead — the same rule color
/// quantization applies, which is why the color-cluster frontend never shows
/// this.
///
/// Only pixels whose color is a *mixture* of two adjacent region means may
/// flip (`d(p,A) + d(p,B) ≤ d(A,B) + slack`): a pixel of a genuine third
/// color — say a dark outline stroke absorbed into a lighter region — must
/// stay with its basin even when some other neighbour's mean happens to sit
/// closer. The mixture pair is usually the pixel's own region and the flip
/// candidate (the classic AA ramp), but a pair of *neighbouring* regions
/// also qualifies: on a blurred low-contrast crack the basin cut can leak a
/// distant region along the crack's blend band as a 1-px filament — those
/// pixels blend the two flanking regions and are unrelated to their own
/// region's color, and they belong to the closer flank.
/// Sweeps are double-buffered (flips apply after scanning) and each moves the
/// boundary at most 1 px, so total movement stays within the ambiguity band;
/// regions are never emptied. Only the first sweep scans the whole canvas;
/// later sweeps revisit the moving front (last sweep's flips and their
/// neighbours), so the cost past sweep one is proportional to the boundary
/// that is actually moving.
fn snap_boundaries(
    img: &ColorImage,
    w: usize,
    h: usize,
    labels: &mut [u32],
    area: &mut [u64],
    sum: &mut [[u64; 3]],
) {
    let n = w * h;
    let k = area.len();
    if k < 2 {
        return;
    }
    // Where a boundary pixel should move, if anywhere: strict improvement
    // only, gated on the mixture test; the first of the fixed neighbour
    // order wins ties, keeping the sweep deterministic.
    let snap_target = |i: usize, labels: &[u32], mean: &[[i32; 3]]| -> Option<u32> {
        let a = labels[i] as usize;
        // Neighbour labels, replicated at the canvas border (a no-op
        // candidate) so the hot path below stays branch-light.
        let (x, y) = (i % w, i / w);
        let nb = [
            labels[if x > 0 { i - 1 } else { i }] as usize,
            labels[if x + 1 < w { i + 1 } else { i }] as usize,
            labels[if y > 0 { i - w } else { i }] as usize,
            labels[if y + 1 < h { i + w } else { i }] as usize,
        ];
        if nb == [a; 4] {
            return None; // interior pixel — the overwhelmingly common case
        }
        let c = img.get_pixel(x, y);
        let cv = [c.r as i32, c.g as i32, c.b as i32];
        let dist = |m: &[i32; 3]| {
            (cv[0] - m[0]).abs() + (cv[1] - m[1]).abs() + (cv[2] - m[2]).abs()
        };
        let da = dist(&mean[a]);
        // The pixel qualifies as a blend of regions `p` and `q` when its
        // color sits between their means (L1 between-ness plus noise slack).
        let mixture = |p: usize, q: usize| -> bool {
            let dpq: i32 = (0..3).map(|ch| (mean[p][ch] - mean[q][ch]).abs()).sum();
            dist(&mean[p]) + dist(&mean[q]) <= dpq + SNAP_SLACK
        };
        let mut best = (da, a);
        for b in nb {
            if b == a {
                continue;
            }
            let db = dist(&mean[b]);
            if db >= best.0 {
                continue;
            }
            if mixture(a, b) || nb.iter().any(|&c| c != a && c != b && mixture(c, b)) {
                best = (db, b);
            }
        }
        (best.1 != a).then_some(best.1 as u32)
    };

    let mut mean = vec![[0i32; 3]; k];
    let mut flips: Vec<(u32, u32)> = Vec::new(); // (pixel, new label)
    let mut front: Vec<u32> = Vec::new(); // pixels to rescan; sweep 0 scans all
    let mut touched: Vec<u32> = Vec::new(); // every front, for the fragment check
    for sweep in 0..SNAP_SWEEPS {
        for r in 0..k {
            for ch in 0..3 {
                mean[r][ch] = (sum[r][ch] / area[r]) as i32;
            }
        }
        flips.clear();
        if sweep == 0 {
            // Interior first with a branch-free neighbour check (the div/mod
            // and border branches in snap_target would dominate a whole-canvas
            // scan), then the border rim.
            for y in 1..h.saturating_sub(1) {
                for i in y * w + 1..y * w + w.saturating_sub(1) {
                    let a = labels[i];
                    if labels[i - 1] == a
                        && labels[i + 1] == a
                        && labels[i - w] == a
                        && labels[i + w] == a
                    {
                        continue;
                    }
                    if let Some(b) = snap_target(i, labels, &mean) {
                        flips.push((i as u32, b));
                    }
                }
            }
            let h1 = h.saturating_sub(1);
            let rim = (0..w)
                .chain((1..h1).map(|y| y * w))
                .chain((1..h1).map(|y| y * w + w - 1).filter(|_| w > 1))
                .chain(if h > 1 { h1 * w..n } else { 0..0 });
            for i in rim {
                if let Some(b) = snap_target(i, labels, &mean) {
                    flips.push((i as u32, b));
                }
            }
        } else {
            for &i in &front {
                if let Some(b) = snap_target(i as usize, labels, &mean) {
                    flips.push((i, b));
                }
            }
        }
        if flips.is_empty() {
            break;
        }
        for &(i, b) in &flips {
            let (i, b) = (i as usize, b as usize);
            let a = labels[i] as usize;
            if area[a] <= 1 {
                continue; // never empty a region
            }
            let c = img.get_pixel(i % w, i / w);
            labels[i] = b as u32;
            area[a] -= 1;
            area[b] += 1;
            for (ch, v) in [c.r, c.g, c.b].into_iter().enumerate() {
                sum[a][ch] -= v as u64;
                sum[b][ch] += v as u64;
            }
        }
        // Next sweep revisits each flipped pixel and its 4-neighbourhood,
        // in raster order for determinism; the same set seeds the fragment
        // check below (a severed strand is always adjacent to the flipped
        // bridge pixel that cut it off).
        front.clear();
        for &(i, _) in &flips {
            let i = i as usize;
            let (x, y) = (i % w, i / w);
            front.push(i as u32);
            if x > 0 {
                front.push((i - 1) as u32);
            }
            if x + 1 < w {
                front.push((i + 1) as u32);
            }
            if y > 0 {
                front.push((i - w) as u32);
            }
            if y + 1 < h {
                front.push((i + w) as u32);
            }
        }
        front.sort_unstable();
        front.dedup();
        touched.extend_from_slice(&front);
    }
    touched.sort_unstable();
    touched.dedup();
    absorb_fragments(img, w, h, labels, area, sum, &touched);
}

/// Fragments a snap flip may pinch off: a pixel can flip toward a neighbour
/// whose own flip then strands it, and a flipped bridge pixel can sever a
/// thin strand of its source region. Watershed basins are connected by
/// construction and everything downstream relies on regions staying coherent
/// (the mosaic gives every disjoint patch its own face), so the snap must not
/// leave debris: a connected component that is disconnected from the rest of
/// its region and fits under this floor is re-assigned to the most
/// color-similar adjacent region. (A *substantial* patch severed at a thin
/// antialiased neck stays — it makes a coherent face of its own; recoloring
/// it would be visible.)
const SNAP_FRAGMENT_MAX: usize = SNAP_SWEEPS * SNAP_SWEEPS;

fn absorb_fragments(
    img: &ColorImage,
    w: usize,
    h: usize,
    labels: &mut [u32],
    area: &mut [u64],
    sum: &mut [[u64; 3]],
    seeds: &[u32],
) {
    let n = w * h;
    let mut visited = vec![false; n];
    let mut comp: Vec<usize> = Vec::new();
    let mut rim: Vec<u32> = Vec::new(); // adjacent region labels
    for &s in seeds {
        let s = s as usize;
        if visited[s] {
            continue;
        }
        // Flood s's same-label component, capped: hitting the cap — or a
        // pixel already visited by an earlier over-cap flood of the same
        // component — proves it is no fragment.
        let l = labels[s];
        visited[s] = true;
        comp.clear();
        comp.push(s);
        rim.clear();
        let mut over = false;
        let mut qi = 0;
        'flood: while qi < comp.len() {
            let i = comp[qi];
            qi += 1;
            let (x, y) = (i % w, i / w);
            for j in [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ]
            .into_iter()
            .flatten()
            {
                if labels[j] != l {
                    rim.push(labels[j]);
                    continue;
                }
                if visited[j] {
                    if !comp.contains(&j) {
                        over = true; // joined an earlier over-cap flood
                        break 'flood;
                    }
                    continue;
                }
                if comp.len() > SNAP_FRAGMENT_MAX {
                    over = true;
                    break 'flood;
                }
                visited[j] = true;
                comp.push(j);
            }
        }
        // A component as large as its whole region is the region itself, not
        // a fragment of one. (The flood can end at cap + 1 without tripping
        // `over`, so re-check the size.)
        if over
            || comp.len() > SNAP_FRAGMENT_MAX
            || comp.len() as u64 >= area[l as usize]
            || rim.is_empty()
        {
            continue;
        }
        // The whole fragment moves to the adjacent region whose mean is
        // closest to the fragment's own mean.
        let mut fsum = [0i64; 3];
        for &i in &comp {
            let c = img.get_pixel(i % w, i / w);
            for (ch, v) in [c.r, c.g, c.b].into_iter().enumerate() {
                fsum[ch] += v as i64;
            }
        }
        let fl = comp.len() as i64;
        rim.sort_unstable();
        rim.dedup();
        let target = rim
            .iter()
            .map(|&b| {
                let d: i64 = (0..3)
                    .map(|ch| {
                        (fsum[ch] / fl - (sum[b as usize][ch] / area[b as usize]) as i64).abs()
                    })
                    .sum();
                (d, b)
            })
            .min()
            .unwrap()
            .1 as usize;
        let l = l as usize;
        for &i in &comp {
            let c = img.get_pixel(i % w, i / w);
            labels[i] = target as u32;
            area[l] -= 1;
            area[target] += 1;
            for (ch, v) in [c.r, c.g, c.b].into_iter().enumerate() {
                sum[l][ch] -= v as u64;
                sum[target][ch] += v as u64;
            }
        }
    }
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

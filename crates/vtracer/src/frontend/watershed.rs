//! Hierarchical watershed frontend — region forming on the pixel graph.
//!
//! The image is treated as a 4-adjacency edge-weighted graph (edge weight =
//! color difference between the two pixels; no gradient image is built). On it
//! we compute the watershed hierarchy by **volume extinction** and cut it at a
//! detail level, following:
//!
//! * Cousty, Bertrand, Najman, Couprie, *Watershed Cuts: Minimum Spanning
//!   Forests and the Drop of Water Principle*, IEEE TPAMI 31(8), 2009.
//! * Najman, Cousty, Perret, *Playing with Kruskal: Algorithms for
//!   Morphological Trees in Edge-Weighted Graphs*, ISMM 2013.
//!
//! Pipeline: Kruskal over counting-sorted edges builds the binary partition
//! tree (a flat `parents` array, leaves `0..n`, internal nodes created in
//! altitude order). A leaves-to-root pass computes each subtree's area and
//! volume; each internal node's *persistence* (the volume of the smaller of
//! the two merged regions) becomes the saliency of its MST edge. Cutting the
//! hierarchy at level λ is then single-linkage over MST edges with
//! persistence ≤ λ — every pixel gets a label, no watershed-line pixels.
//!
//! Everything is integer and allocation-flat: counting sort over 256 weight
//! buckets, path-halving union-find, `u32` node ids. Deterministic across
//! platforms.

use visioncortex::{BinaryImage, Color, ColorImage, PointI32};

use crate::error::Error;
use crate::ir::{Layer, Paint, RegionMask, Segmentation};

use super::Frontend;

/// Watershed frontend: hierarchical watershed by volume, cut at `detail`.
#[derive(Debug, Clone)]
pub struct WatershedFrontend {
    /// Detail level (0..=255): where to cut the hierarchy. 255 keeps every
    /// basin that survives a zero-persistence merge (finest useful partition);
    /// 0 merges everything into a single region.
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

impl WatershedFrontend {
    fn label_map(&self, img: &ColorImage) -> Vec<u32> {
        let w = img.width;
        let h = img.height;
        let n = w * h;

        // --- 4-adjacency edges, counting-sorted by weight -------------------
        // Edge id encodes (pixel, direction): 2*p = right, 2*p+1 = down.
        // The per-bucket fill preserves edge-id order, so the sort is stable
        // and the whole construction is deterministic.
        let px = |i: usize| {
            let c = img.get_pixel(i % w, i / w);
            c
        };
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
        let mut order = vec![0u32; n_edges];
        let mut fill = start;
        for i in 0..n {
            if i % w + 1 < w {
                let e = 2 * i;
                let b = weight_of[e] as usize;
                order[fill[b]] = e as u32;
                fill[b] += 1;
            }
            if i / w + 1 < h {
                let e = 2 * i + 1;
                let b = weight_of[e] as usize;
                order[fill[b]] = e as u32;
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
        let mut mst_edge = vec![(0u32, 0u32); n - 1]; // pixel pair of edge k
        let mut uf = Uf::new(n);
        // Current tree node representing each union-find root's component.
        let mut comp_node: Vec<u32> = (0..n as u32).collect();
        let mut next = n as u32;
        for &e in &order {
            let p = (e / 2) as usize;
            let q = if e % 2 == 0 { p + 1 } else { p + w };
            let (rp, rq) = (uf.find(p as u32), uf.find(q as u32));
            if rp == rq {
                continue;
            }
            let k = (next - n as u32) as usize;
            alt[next as usize] = weight_of[e as usize];
            child[k] = [comp_node[rp as usize], comp_node[rq as usize]];
            mst_edge[k] = (p as u32, q as u32);
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

        // --- Cut level from the detail slider --------------------------------
        // Merging every MST edge with persistence ≤ λ leaves exactly
        // 1 + #{edges above λ} regions, so choosing λ as the k-th largest
        // persistence targets k regions directly (ties merge a little more).
        // The persistence distribution is extremely skewed — most merges are
        // trivia with persistence ≈ 0 — so the slider maps to a region
        // *count*, exponentially: every +25.5 of detail doubles the target,
        // from 1 region at 0 up to 1024 at 255.
        let target = (2f64).powf(self.detail as f64 / 25.5).round() as usize;
        let target = target.clamp(1, pers.len());
        let lambda = {
            let mut sorted = pers.clone();
            sorted.sort_unstable_by(|a, b| b.cmp(a));
            sorted[target - 1]
        };

        // --- Single-linkage cut over MST edges -------------------------------
        let mut cut = Uf::new(n);
        for k in 0..n - 1 {
            if pers[k] <= lambda {
                let (p, q) = mst_edge[k];
                let (rp, rq) = (cut.find(p), cut.find(q));
                if rp != rq {
                    cut.link(rp, rq);
                }
            }
        }
        let mut labels = vec![0u32; n];
        for (i, l) in labels.iter_mut().enumerate() {
            *l = cut.find(i as u32);
        }
        labels
    }

    /// Absorb regions smaller than `min_area` into their most color-similar
    /// 4-neighbour. Works on root-labels in place; areas and color sums are
    /// maintained through the merges so chains stay well-behaved.
    fn absorb_small(&self, img: &ColorImage, labels: &mut [u32]) {
        if self.min_area <= 1 {
            return;
        }
        let w = img.width;
        let n = labels.len();
        let mut uf = Uf::new(n);
        // Rebuild region stats keyed by current label (a pixel index).
        let mut area = vec![0u64; n];
        let mut sum = vec![[0u64; 3]; n];
        for i in 0..n {
            let l = labels[i] as usize;
            let c = img.get_pixel(i % w, i / w);
            area[l] += 1;
            sum[l][0] += c.r as u64;
            sum[l][1] += c.g as u64;
            sum[l][2] += c.b as u64;
        }
        let mean_diff = |sa: &[u64; 3], aa: u64, sb: &[u64; 3], ab: u64| -> u64 {
            let mut d = 0i64;
            for ch in 0..3 {
                d += ((sa[ch] / aa) as i64 - (sb[ch] / ab) as i64).abs();
            }
            d as u64
        };
        // Sweep until no undersized region can be absorbed. Each sweep scans
        // the boundary edges once and merges each small region into its best
        // neighbour seen so far; region count strictly decreases, so this
        // terminates quickly in practice.
        loop {
            // best[l] = (diff, neighbour_root) for undersized root l
            let mut best: Vec<(u64, u32)> = vec![(u64::MAX, u32::MAX); n];
            let mut any_small = false;
            for i in 0..n {
                let a = uf.find(labels[i]);
                for j in [
                    if i % w + 1 < w { i + 1 } else { i },
                    if i / w + 1 < labels.len() / w { i + w } else { i },
                ] {
                    if j == i {
                        continue;
                    }
                    let b = uf.find(labels[j]);
                    if a == b {
                        continue;
                    }
                    for (s, t) in [(a, b), (b, a)] {
                        let (su, tu) = (s as usize, t as usize);
                        if area[su] < self.min_area as u64 {
                            any_small = true;
                            let d = mean_diff(&sum[su], area[su], &sum[tu], area[tu]);
                            if d < best[su].0 || (d == best[su].0 && t < best[su].1) {
                                best[su] = (d, t);
                            }
                        }
                    }
                }
            }
            if !any_small {
                break;
            }
            let mut merged = false;
            for l in 0..n {
                let (_, tgt) = best[l];
                if tgt == u32::MAX {
                    continue;
                }
                let rl = uf.find(l as u32);
                if rl as usize != l {
                    continue; // already absorbed this sweep
                }
                let rt = uf.find(tgt);
                if rt == rl {
                    continue;
                }
                uf.link(rt, rl);
                area[rt as usize] += area[l];
                for ch in 0..3 {
                    sum[rt as usize][ch] += sum[l][ch];
                }
                merged = true;
            }
            if !merged {
                break; // isolated undersized region (e.g. whole-canvas)
            }
        }
        for l in labels.iter_mut() {
            *l = uf.find(*l);
        }
    }

    /// Turn a root-label map into the layered [`Segmentation`]: one layer per
    /// region with its mean color, the largest region first as a solid
    /// full-canvas background so stacked mode stays seam-free by overdraw.
    fn segmentation(img: &ColorImage, labels: &[u32]) -> Segmentation {
        let w = img.width;
        let h = img.height;
        let n = labels.len();

        // Compact labels in raster order of first appearance (deterministic).
        let mut compact = vec![u32::MAX; n];
        let mut regions: Vec<u32> = Vec::new(); // compact id -> root label
        let mut ids = vec![0u32; n];
        for i in 0..n {
            let l = labels[i] as usize;
            if compact[l] == u32::MAX {
                compact[l] = regions.len() as u32;
                regions.push(labels[i]);
            }
            ids[i] = compact[l];
        }
        let m = regions.len();

        let mut area = vec![0u64; m];
        let mut sum = vec![[0u64; 3]; m];
        let mut bbox = vec![(i32::MAX, i32::MAX, i32::MIN, i32::MIN); m];
        for i in 0..n {
            let id = ids[i] as usize;
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            let c = img.get_pixel(i % w, i / w);
            area[id] += 1;
            sum[id][0] += c.r as u64;
            sum[id][1] += c.g as u64;
            sum[id][2] += c.b as u64;
            let b = &mut bbox[id];
            b.0 = b.0.min(x);
            b.1 = b.1.min(y);
            b.2 = b.2.max(x);
            b.3 = b.3.max(y);
        }
        let mean = |id: usize| {
            Color::new(
                (sum[id][0] / area[id]) as u8,
                (sum[id][1] / area[id]) as u8,
                (sum[id][2] / area[id]) as u8,
            )
        };

        let background = (0..m).max_by_key(|&id| area[id]).unwrap_or(0);

        let mut seg = Segmentation::new(w as u32, h as u32);
        // Background: solid full canvas, painted first; the regions stacked on
        // top stamp out everything that isn't actually background, so the
        // flattened partition is exact while stacked mode keeps overdraw.
        let mut bg = BinaryImage::new_w_h(w, h);
        for y in 0..h {
            for x in 0..w {
                bg.set_pixel(x, y, true);
            }
        }
        seg.layers.push(Layer {
            paint: Paint::Solid(mean(background)),
            mask: RegionMask::new(bg, PointI32 { x: 0, y: 0 }),
        });
        for id in 0..m {
            if id == background {
                continue;
            }
            let (x0, y0, x1, y1) = bbox[id];
            let (bw, bh) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
            let mut image = BinaryImage::new_w_h(bw, bh);
            for y in 0..bh {
                for x in 0..bw {
                    let i = (y0 as usize + y) * w + (x0 as usize + x);
                    if ids[i] as usize == id {
                        image.set_pixel(x, y, true);
                    }
                }
            }
            seg.layers.push(Layer {
                paint: Paint::Solid(mean(id)),
                mask: RegionMask::new(image, PointI32 { x: x0, y: y0 }),
            });
        }
        seg
    }
}

impl Frontend for WatershedFrontend {
    fn segment(&self, img: &ColorImage) -> Result<Segmentation, Error> {
        if img.width == 0 || img.height == 0 {
            return Err(Error::EmptyImage);
        }
        if img.width * img.height == 1 {
            // Degenerate single pixel: no edges, one region.
            let labels = [0u32];
            return Ok(Self::segmentation(img, &labels));
        }

        let mut labels = self.label_map(img);
        self.absorb_small(img, &mut labels);
        Ok(Self::segmentation(img, &labels))
    }
}

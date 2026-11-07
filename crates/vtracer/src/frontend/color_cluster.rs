//! Fast, O(N) bucket-indexed hierarchical color-clustering frontend.
//!
//! Replaces `visioncortex::color_clusters::Runner` with a direct, allocation-flat
//! implementation of the exact same two-stage algorithm:
//!
//! 1. **Stage 1 (Scanline clustering)**: group adjacent pixels with similar
//!    quantized colors (`color_same`).
//! 2. **Stage 2 (Hierarchical merging by ascending area)**: iterate cluster
//!    areas in ascending order, merge small/similar patches into their closest
//!    color neighbor (`color_diff`), and emit layers (`deepen`) when a good
//!    patch differs from its closest neighbor by more than `layer_difference`.
//!
//! Key algorithmic improvements over `visioncortex`:
//! * **Area buckets (`BTreeMap<usize, Vec<u32>>`)**: instead of linearly scanning
//!   all `0..clusters.len()` at *every* distinct area step (which was $O(C \times A)
//!   \approx O(N^{1.5})$ on photographs), we pop the bucket for `cur_area` in $O(1)$.
//! * **Zero-HashSet neighbour discovery**: instead of allocating a `HashSet` per
//!   cluster per merge step, a reusable `seen_stamp` array deduplicates neighbour
//!   cluster IDs in $O(\text{boundary})$ with zero heap allocation.
//! * **Direct boundary perimeter count**: `patch_good` counts boundary pixels
//!   directly from `cluster_indices` + `holes` instead of allocating TWO full
//!   `BinaryImage`s (`to_image` + `Shape::image_boundary_list`) per candidate!

use std::collections::BTreeMap;

use visioncortex::{BinaryImage, BoundingRect, Color, ColorImage, ColorSum, PointI32};

use crate::error::Error;
use crate::ir::{Layer, Paint, RegionMask, Segmentation};
use crate::progress::{Ctx, Phase};

use super::keying::{apply_key, find_unused_color, should_key_image};
use super::Frontend;

const ZERO: u32 = 0;

/// Hierarchical color-clustering frontend — the classic VTracer color path.
///
/// Speckle removal happens *inside* clustering, via `good_min_area`: it is the
/// clusterer's `deepen` gate (`patch_good`), so it does far more
/// than drop small regions — it decides whether a small/thin patch is absorbed
/// into its neighbor (its color averaged in) or kept as its own layer. Forcing
/// it to 0 disables the thread-like rejection and changes the whole hierarchy,
/// so speckle must be a clustering parameter, not a downstream filter.
#[derive(Debug, Clone)]
pub struct ColorClusterFrontend {
    /// Bits of color precision dropped when comparing pixels (0 = full 8-bit).
    pub color_precision_loss: i32,
    /// Color difference between hierarchical gradient layers.
    pub layer_difference: i32,
    /// Minimum area (px) for a patch to be a `deepen` candidate during
    /// clustering; non-zero also enables thread-like rejection.
    /// Below it, patches are absorbed into their nearest-color neighbor.
    pub good_min_area: usize,
}

impl Default for ColorClusterFrontend {
    fn default() -> Self {
        Self {
            color_precision_loss: 2,
            layer_difference: 16,
            good_min_area: 0,
        }
    }
}

#[derive(Clone, Default)]
struct InternalCluster {
    indices: Vec<u32>,
    holes: Vec<u32>,
    num_holes: u32,
    depth: u32,
    sum: ColorSum,
    residue_sum: ColorSum,
    rect: BoundingRect,
    merged_into: u32,
}

impl InternalCluster {
    #[inline]
    fn add(&mut self, i: u32, color: &Color, x: i32, y: i32) {
        self.indices.push(i);
        self.sum.add(color);
        self.rect.add_x_y(x, y);
    }

    #[inline]
    fn area(&self) -> usize {
        self.indices.len()
    }

    #[inline]
    fn color(&self) -> Color {
        self.sum.average()
    }

    #[inline]
    fn residue_color(&self) -> Color {
        self.residue_sum.average()
    }

    fn to_image_with_hole(&self, parent_width: u32, hole: bool) -> BinaryImage {
        let width = self.rect.width() as usize;
        let height = self.rect.height() as usize;
        let mut image = BinaryImage::new_w_h(width, height);
        let left = self.rect.left as usize;
        let top = self.rect.top as usize;
        let pw = parent_width as usize;

        for &i in &self.indices {
            let x = (i as usize % pw) - left;
            let y = (i as usize / pw) - top;
            image.set_pixel_index(y * width + x, true);
        }

        if hole {
            for &i in &self.holes {
                let x = (i as usize % pw) - left;
                let y = (i as usize / pw) - top;
                image.set_pixel_index(y * width + x, false);
            }
        }

        image
    }
}

#[inline]
fn color_diff(a: Color, b: Color) -> i32 {
    (a.r as i32 - b.r as i32).abs()
        + (a.g as i32 - b.g as i32).abs()
        + (a.b as i32 - b.b as i32).abs()
}

#[inline]
fn color_same(a: Color, b: Color, shift: i32, thres: i32) -> bool {
    ((a.r >> shift) as i32 - (b.r >> shift) as i32).abs() <= thres
        && ((a.g >> shift) as i32 - (b.g >> shift) as i32).abs() <= thres
        && ((a.b >> shift) as i32 - (b.b >> shift) as i32).abs() <= thres
}

struct FastRunner {
    diagonal: bool,
    batch_size: u32,
    good_min_area: usize,
    good_max_area: usize,
    shift: i32,
    deepen_diff: i32,
    hollow_neighbours: usize,
    key: Color,
    has_key: bool,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    clusters: Vec<InternalCluster>,
    cluster_indices: Vec<u32>,
    /// Bucketed clusters by area: `area -> list of cluster indices in ascending ID order`.
    area_buckets: BTreeMap<usize, Vec<u32>>,
    clusters_output: Vec<u32>,
    stage: u32,
    iteration: u32,
    total_stage2_areas: u32,
    done_stage2_areas: u32,
    next_index: u32,
    /// Reusable stamp buffer for O(1) neighbour deduplication and hole-aware perimeter.
    stamp: Vec<u32>,
    stamp_gen: u32,
    /// Reusable pixel-hole stamp buffer (sized `width * height`, lazily allocated).
    hole_stamp: Vec<u32>,
    hole_gen: u32,
}

impl FastRunner {
    fn new(
        img: ColorImage,
        color_precision_loss: i32,
        layer_difference: i32,
        good_min_area: usize,
        key_color: Color,
    ) -> Self {
        let width = img.width as u32;
        let height = img.height as u32;
        let n = img.width * img.height;
        Self {
            diagonal: layer_difference == 0,
            batch_size: 25600,
            good_min_area,
            good_max_area: n,
            shift: color_precision_loss,
            deepen_diff: layer_difference,
            hollow_neighbours: 1,
            key: key_color,
            has_key: key_color != Color::default(),
            width,
            height,
            pixels: img.pixels,
            clusters: vec![InternalCluster::default()],
            cluster_indices: vec![ZERO; n],
            area_buckets: BTreeMap::new(),
            clusters_output: Vec::new(),
            stage: 1,
            iteration: 0,
            total_stage2_areas: 1,
            done_stage2_areas: 0,
            next_index: 1,
            stamp: Vec::new(),
            stamp_gen: 0,
            hole_stamp: Vec::new(),
            hole_gen: 0,
        }
    }

    #[inline]
    fn get_pixel(&self, i: u32) -> Color {
        let b = i as usize * 4;
        Color::new_rgba(
            self.pixels[b],
            self.pixels[b + 1],
            self.pixels[b + 2],
            self.pixels[b + 3],
        )
    }

    fn tick(&mut self) -> bool {
        match self.stage {
            1 => {
                if self.stage_1() {
                    self.stage = 2;
                    self.iteration = 0;
                }
                false
            }
            2 => {
                let steps = std::cmp::max(1, self.done_stage2_areas / 16);
                for _ in 0..steps {
                    if self.stage_2() {
                        self.stage = 3;
                        break;
                    }
                }
                false
            }
            _ => true,
        }
    }

    fn progress(&self) -> u32 {
        match self.stage {
            1 => 50 * self.iteration / (self.cluster_indices.len().max(1) as u32),
            2 => 50 + 50 * self.done_stage2_areas / self.total_stage2_areas.max(1),
            _ => 100,
        }
    }

    fn stage_1(&mut self) -> bool {
        let diagonal = self.diagonal;
        let batch_size = self.batch_size;
        let key = self.key;
        let has_key = self.has_key;
        let shift = self.shift;
        let w = self.width;
        let len = self.cluster_indices.len() as u32;
        let end = (self.iteration + batch_size).min(len);

        for i in self.iteration..end {
            let x = i % w;
            let y = i / w;

            let c = self.get_pixel(i);
            let up = if y > 0 { Some(self.get_pixel(i - w)) } else { None };
            let left = if x > 0 { Some(self.get_pixel(i - 1)) } else { None };
            let upleft = if x > 0 && y > 0 {
                Some(self.get_pixel(i - w - 1))
            } else {
                None
            };

            let mut cluster_up = if y > 0 {
                self.cluster_indices[(i - w) as usize]
            } else {
                ZERO
            };
            let mut cluster_left = if x > 0 {
                self.cluster_indices[(i - 1) as usize]
            } else {
                ZERO
            };
            let cluster_upleft = if x > 0 && y > 0 {
                self.cluster_indices[(i - w - 1) as usize]
            } else {
                ZERO
            };

            let same_up_left = match (left, up) {
                (Some(l), Some(u)) => color_same(l, u, shift, 1),
                _ => false,
            };
            let same_c_up = up.is_some_and(|u| color_same(c, u, shift, 1));
            let same_c_left = left.is_some_and(|l| color_same(c, l, shift, 1));
            let same_c_upleft = upleft.is_some_and(|ul| color_same(c, ul, shift, 1));

            if cluster_left != cluster_up
                && same_up_left
                && (diagonal || (same_c_left && same_c_up))
            {
                if self.clusters[cluster_left as usize].area()
                    <= self.clusters[cluster_up as usize].area()
                {
                    self.combine_clusters(cluster_left, cluster_up);
                    if cluster_left == self.next_index - 1
                        && self.next_index as usize == self.clusters.len()
                    {
                        self.next_index -= 1;
                    }
                    cluster_left = cluster_up;
                } else {
                    self.combine_clusters(cluster_up, cluster_left);
                    cluster_up = cluster_left;
                }
            }

            if has_key && c == key {
                // KeyingAction::Discard
            } else if same_c_up && same_c_upleft {
                self.cluster_indices[i as usize] = cluster_up;
                self.clusters[cluster_up as usize].add(i, &c, x as i32, y as i32);
            } else if same_c_left && same_c_upleft {
                self.cluster_indices[i as usize] = cluster_left;
                self.clusters[cluster_left as usize].add(i, &c, x as i32, y as i32);
            } else if diagonal && same_c_upleft {
                self.cluster_indices[i as usize] = cluster_upleft;
                self.clusters[cluster_upleft as usize].add(i, &c, x as i32, y as i32);
            } else {
                let idx = self.next_index as usize;
                if idx < self.clusters.len() {
                    let cl = &mut self.clusters[idx];
                    cl.indices.clear();
                    cl.holes.clear();
                    cl.num_holes = 0;
                    cl.depth = 0;
                    cl.sum.clear();
                    cl.residue_sum.clear();
                    cl.rect.clear();
                    cl.merged_into = 0;
                    cl.add(i, &c, x as i32, y as i32);
                } else {
                    let mut new_cluster = InternalCluster::default();
                    new_cluster.add(i, &c, x as i32, y as i32);
                    self.clusters.push(new_cluster);
                }
                self.cluster_indices[i as usize] = self.next_index;
                self.next_index += 1;
            }
        }

        self.iteration = end;
        if self.iteration >= len {
            self.prepare_stage_2();
            true
        } else {
            false
        }
    }

    fn prepare_stage_2(&mut self) {
        for (idx, c) in self.clusters.iter_mut().enumerate() {
            c.residue_sum = c.sum;
            let a = c.area();
            if a > 0 {
                self.area_buckets.entry(a).or_default().push(idx as u32);
            }
        }
        self.total_stage2_areas = self.area_buckets.len().max(1) as u32;
        self.done_stage2_areas = 0;
        self.stamp.resize(self.clusters.len(), 0);
    }

    /// Collect sorted unique neighbour cluster indices of `index` in O(cluster_area)
    /// without allocating a `HashSet`.
    fn collect_neighbours(&mut self, index: u32, out: &mut Vec<u32>) {
        out.clear();
        let indices = &self.clusters[index as usize].indices;
        if indices.is_empty() {
            return;
        }
        let myself = self.cluster_indices[indices[0] as usize];
        self.stamp_gen = self.stamp_gen.wrapping_add(1);
        if self.stamp_gen == 0 {
            self.stamp.fill(0);
            self.stamp_gen = 1;
        }
        let cur_gen = self.stamp_gen;
        let w = self.width;
        let h = self.height;

        for &i in indices {
            let x = i % w;
            let y = i / w;
            if y > 0 {
                let nb = self.cluster_indices[(i - w) as usize];
                if nb != ZERO && nb != myself && self.stamp[nb as usize] != cur_gen {
                    self.stamp[nb as usize] = cur_gen;
                    out.push(nb);
                }
            }
            if y + 1 < h {
                let nb = self.cluster_indices[(i + w) as usize];
                if nb != ZERO && nb != myself && self.stamp[nb as usize] != cur_gen {
                    self.stamp[nb as usize] = cur_gen;
                    out.push(nb);
                }
            }
            if x > 0 {
                let nb = self.cluster_indices[(i - 1) as usize];
                if nb != ZERO && nb != myself && self.stamp[nb as usize] != cur_gen {
                    self.stamp[nb as usize] = cur_gen;
                    out.push(nb);
                }
            }
            if x + 1 < w {
                let nb = self.cluster_indices[(i + 1) as usize];
                if nb != ZERO && nb != myself && self.stamp[nb as usize] != cur_gen {
                    self.stamp[nb as usize] = cur_gen;
                    out.push(nb);
                }
            }
        }
        out.sort_unstable();
    }

    /// Exact equivalent of `visioncortex::Cluster::perimeter` (`Shape::image_boundary_list(&self.to_image(parent)).len()`)
    /// without allocating two `BinaryImage`s!
    ///
    /// In `to_image(parent)` (`hole = true`), the foreground pixels are `indices \ holes`.
    /// During Stage 2, every pixel in `indices` currently has `cluster_indices[p] == index`.
    /// So a pixel `q` is in `indices \ holes` iff `cluster_indices[q] == index` and `q` is not in `holes`!
    fn cluster_perimeter(&mut self, index: u32) -> usize {
        let cluster = &self.clusters[index as usize];
        if cluster.indices.is_empty() {
            return 0;
        }
        let has_holes = !cluster.holes.is_empty();
        if has_holes {
            if self.hole_stamp.is_empty() {
                self.hole_stamp.resize(self.cluster_indices.len(), 0);
            }
            self.hole_gen = self.hole_gen.wrapping_add(1);
            if self.hole_gen == 0 {
                self.hole_stamp.fill(0);
                self.hole_gen = 1;
            }
            let hgen = self.hole_gen;
            for &hp in &self.clusters[index as usize].holes {
                self.hole_stamp[hp as usize] = hgen;
            }
        }

        let hgen = self.hole_gen;
        let w = self.width;
        let h = self.height;
        let mut perim = 0usize;

        for &i in &self.clusters[index as usize].indices {
            let iu = i as usize;
            if has_holes && self.hole_stamp[iu] == hgen {
                continue;
            }
            let x = i % w;
            let y = i / w;
            let is_fg = |ni: usize| -> bool {
                self.cluster_indices[ni] == index && (!has_holes || self.hole_stamp[ni] != hgen)
            };
            if x == 0
                || x + 1 == w
                || y == 0
                || y + 1 == h
                || !is_fg(iu - 1)
                || !is_fg(iu + 1)
                || !is_fg(iu - w as usize)
                || !is_fg(iu + w as usize)
            {
                perim += 1;
            }
        }
        perim
    }

    fn patch_good(&mut self, index: u32) -> bool {
        let area = self.clusters[index as usize].area();
        if self.good_min_area < area && area < self.good_max_area {
            if self.good_min_area == 0 || self.cluster_perimeter(index) < area {
                return true;
            }
        }
        false
    }

    fn stage_2(&mut self) -> bool {
        let Some((&cur_area, _)) = self.area_buckets.first_key_value() else {
            return true;
        };
        let mut bucket = self.area_buckets.pop_first().unwrap().1;
        self.done_stage2_areas += 1;
        // Multiple merges during earlier steps can push new entries into `bucket` out of order;
        // sorting by cluster index preserves the exact `0..self.clusters.len()` scan order of visioncortex!
        bucket.sort_unstable();
        bucket.dedup();

        let is_last_area = self.area_buckets.is_empty();
        let can_discard_pixels = self.has_key;
        let mut raw_nbs: Vec<u32> = Vec::new();
        let mut infos: Vec<(i32, u32)> = Vec::new(); // (diff, index)

        let mut bi = 0;
        while bi < bucket.len() {
            let index = bucket[bi];
            bi += 1;

            if self.clusters[index as usize].area() != cur_area {
                continue;
            }

            let mycolor = self.clusters[index as usize].color();
            self.collect_neighbours(index, &mut raw_nbs);

            infos.clear();
            for &other in &raw_nbs {
                if self.clusters[other as usize].sum.counter > 0 {
                    let d = color_diff(mycolor, self.clusters[other as usize].color());
                    infos.push((d, other));
                }
            }

            if infos.is_empty() {
                if is_last_area && bi == bucket.len() || can_discard_pixels {
                    self.clusters_output.push(index);
                }
                continue;
            }

            infos.sort_unstable_by_key(|&(d, idx)| (d as i64) * 65535 + idx as i64);
            let (best_diff, target) = infos[0];

            let deepen = best_diff > self.deepen_diff && self.patch_good(index);
            let hollow = infos.len() <= self.hollow_neighbours;

            if deepen {
                self.clusters_output.push(index);
            }

            self.merge_cluster_into(index, target, deepen, hollow);
            let updated_area = self.clusters[target as usize].area();

            // `updated_area` is strictly greater than `cur_area` (since `cur_area > 0`
            // and `target` had `area >= 1`), so it always lands in a future bucket!
            let is_new_bucket = !self.area_buckets.contains_key(&updated_area);
            self.area_buckets
                .entry(updated_area)
                .or_default()
                .push(target);
            if is_new_bucket {
                self.total_stage2_areas += 1;
            }
        }

        self.area_buckets.is_empty()
    }

    fn merge_cluster_into(&mut self, from: u32, to: u32, deepen: bool, hollow: bool) {
        if !deepen {
            let residue_sum = self.clusters[from as usize].residue_sum;
            self.clusters[to as usize].residue_sum.merge(&residue_sum);
            self.combine_clusters(from, to);
        } else {
            self.combine_clusters_clone(from, to);

            if hollow {
                let mut holes = self.clusters[from as usize].indices.clone();
                self.clusters[to as usize].holes.append(&mut holes);
                self.clusters[to as usize].num_holes += 1;
            }

            self.clusters[from as usize].merged_into = to;
            self.clusters[to as usize].depth += 1;
        }
    }

    fn combine_clusters_clone(&mut self, from: u32, to: u32) {
        let sum = self.clusters[from as usize].sum;
        let rect = self.clusters[from as usize].rect;
        let indices = self.clusters[from as usize].indices.clone();

        self.combine_clusters(from, to);

        self.clusters[from as usize].sum = sum;
        self.clusters[from as usize].rect = rect;
        self.clusters[from as usize].indices = indices;
    }

    fn combine_clusters(&mut self, from: u32, to: u32) {
        for &i in &self.clusters[from as usize].indices {
            self.cluster_indices[i as usize] = to;
        }

        let mut indices = std::mem::take(&mut self.clusters[from as usize].indices);
        self.clusters[to as usize].indices.append(&mut indices);
        let sum = self.clusters[from as usize].sum;
        let rect = self.clusters[from as usize].rect;
        self.clusters[to as usize].sum.merge(&sum);
        self.clusters[to as usize].rect.merge(rect);
        self.clusters[from as usize].sum.clear();
        self.clusters[from as usize].rect.clear();
    }

    fn into_segmentation(mut self) -> Segmentation {
        let width = self.width as usize;
        let height = self.height as usize;
        self.clusters_output
            .retain(|&i| self.clusters[i as usize].sum.counter > 0);

        let mut seg = Segmentation::new(self.width, self.height);
        let mut flat_labels = vec![u32::MAX; width * height];

        let ordered: Vec<u32> = self.clusters_output.iter().rev().copied().collect();
        for (layer_idx, &cluster_index) in ordered.iter().enumerate() {
            let cluster = &self.clusters[cluster_index as usize];
            let lid = layer_idx as u32;
            for &p in &cluster.indices {
                flat_labels[p as usize] = lid;
            }
        }

        let clusters_ref = &self.clusters;
        let w_u32 = self.width;
        let build_layer = |&cluster_index: &u32| {
            let cluster = &clusters_ref[cluster_index as usize];
            let image = cluster.to_image_with_hole(w_u32, false);
            let mask = RegionMask::new(
                image,
                PointI32 {
                    x: cluster.rect.left,
                    y: cluster.rect.top,
                },
            );
            Layer {
                paint: Paint::Solid(cluster.residue_color()),
                mask,
            }
        };
        seg.layers = ordered.iter().map(build_layer).collect();
        seg.flat_labels = Some(flat_labels);
        seg
    }
}

impl ColorClusterFrontend {
    fn prepare_runner(&self, img: &ColorImage) -> Result<FastRunner, Error> {
        if img.width == 0 || img.height == 0 {
            return Err(Error::EmptyImage);
        }

        let (img, key_color) = if should_key_image(img) {
            let key = find_unused_color(img)?;
            let mut img = img.clone();
            apply_key(&mut img, key);
            (img, key)
        } else {
            (img.clone(), Color::default())
        };

        Ok(FastRunner::new(
            img,
            self.color_precision_loss,
            self.layer_difference,
            self.good_min_area,
            key_color,
        ))
    }
}

impl Frontend for ColorClusterFrontend {
    fn segment(&self, img: &ColorImage) -> Result<Segmentation, Error> {
        let mut runner = self.prepare_runner(img)?;
        while !runner.tick() {}
        Ok(runner.into_segmentation())
    }

    fn segment_with(&self, img: &ColorImage, ctx: &mut Ctx) -> Result<Segmentation, Error> {
        let mut runner = self.prepare_runner(img)?;
        ctx.report(Phase::Segment, 0.0);
        while !runner.tick() {
            ctx.check()?;
            ctx.report(Phase::Segment, runner.progress() as f32 / 100.0);
        }
        ctx.check()?;
        ctx.report(Phase::Segment, 1.0);
        Ok(runner.into_segmentation())
    }
}

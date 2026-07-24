use visioncortex::color_clusters::{
    Clusters, KeyingAction, Runner, RunnerConfig, HIERARCHICAL_MAX,
};
use visioncortex::{Color, ColorImage, PointI32};

// (Runner is constructed inline in each entry point so its generic closure
// types never appear in a return signature.)

use crate::error::Error;
use crate::ir::{Layer, Paint, RegionMask, Segmentation};
use crate::progress::{Ctx, Phase};

use super::keying::{apply_key, find_unused_color, should_key_image};
use super::Frontend;

/// Hierarchical color-clustering frontend — the classic VTracer color path.
#[derive(Debug, Clone)]
pub struct ColorClusterFrontend {
    /// Discard clusters smaller than this many pixels.
    pub filter_speckle_area: usize,
    /// Bits of color precision dropped when comparing pixels (0 = full 8-bit).
    pub color_precision_loss: i32,
    /// Color difference between hierarchical gradient layers.
    pub layer_difference: i32,
}

impl Default for ColorClusterFrontend {
    fn default() -> Self {
        Self {
            filter_speckle_area: 16,
            color_precision_loss: 2,
            layer_difference: 16,
        }
    }
}

impl ColorClusterFrontend {
    /// Apply transparency keying (if warranted) and build the clustering
    /// inputs: the keyed image, the `RunnerConfig`, and the dimensions. The
    /// caller constructs `Runner::new(config, image)` inline so the runner's
    /// generic closure types never surface in a return signature.
    fn prepare(&self, img: &ColorImage) -> Result<(ColorImage, RunnerConfig, usize, usize), Error> {
        if img.width == 0 || img.height == 0 {
            return Err(Error::EmptyImage);
        }

        let width = img.width;
        let height = img.height;
        let mut img = img.clone();

        // Transparency keying (stacked mode discards the keyed background).
        let key_color = if should_key_image(&img) {
            let key = find_unused_color(&img)?;
            apply_key(&mut img, key);
            key
        } else {
            // All-zero is the sentinel understood by visioncortex as "no keying".
            Color::default()
        };

        let config = RunnerConfig {
            diagonal: self.layer_difference == 0,
            hierarchical: HIERARCHICAL_MAX,
            batch_size: 25600,
            good_min_area: self.filter_speckle_area,
            good_max_area: width * height,
            is_same_color_a: self.color_precision_loss,
            is_same_color_b: 1,
            deepen_diff: self.layer_difference,
            hollow_neighbours: 1,
            key_color,
            keying_action: KeyingAction::Discard,
        };

        Ok((img, config, width, height))
    }

    /// Turn finished clusters into the layered [`Segmentation`].
    fn segmentation_from_clusters(clusters: &Clusters, width: usize, height: usize) -> Segmentation {
        let view = clusters.view();
        let mut seg = Segmentation::new(width as u32, height as u32);
        // `clusters_output` is top-to-bottom; reverse to get bottom-to-top
        // paint order for the layer stack.
        for &cluster_index in view.clusters_output.iter().rev() {
            let cluster = view.get_cluster(cluster_index);
            // Solid cluster masks (no holes punched): stacked mode relies on
            // paint-order overdraw for occlusion, matching 0.6.x. Punching
            // holes here would leave the layer below exposed as hairline seams.
            // The mosaic flatten is unaffected — a higher layer still wins per
            // pixel — so a solid parent gives the same partition.
            let image = cluster.to_image_with_hole(view.width, false);
            let mask = RegionMask::new(
                image,
                PointI32 {
                    x: cluster.rect.left,
                    y: cluster.rect.top,
                },
            );
            seg.layers.push(Layer {
                paint: Paint::Solid(cluster.residue_color()),
                mask,
            });
        }
        seg
    }
}

impl Frontend for ColorClusterFrontend {
    fn segment(&self, img: &ColorImage) -> Result<Segmentation, Error> {
        let (image, config, width, height) = self.prepare(img)?;
        let clusters = Runner::new(config, image).run();
        Ok(Self::segmentation_from_clusters(&clusters, width, height))
    }

    fn segment_with(&self, img: &ColorImage, ctx: &mut Ctx) -> Result<Segmentation, Error> {
        let (image, config, width, height) = self.prepare(img)?;

        // Drive clustering incrementally so we can publish progress and observe
        // cancellation between batches. `run()` is exactly this loop, so the
        // resulting clusters are identical to the blocking path.
        let mut builder = Runner::new(config, image).start();
        ctx.report(Phase::Segment, 0.0);
        while !builder.tick() {
            ctx.check()?;
            ctx.report(Phase::Segment, builder.progress() as f32 / 100.0);
        }
        ctx.check()?;
        let clusters = builder.result();
        ctx.report(Phase::Segment, 1.0);

        Ok(Self::segmentation_from_clusters(&clusters, width, height))
    }
}

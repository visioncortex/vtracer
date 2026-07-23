use visioncortex::{Color, ColorImage, PointI32};

use crate::error::Error;
use crate::ir::{Layer, Paint, RegionMask, Segmentation};

use super::Frontend;

/// Binary (black/white) frontend: threshold the image then cluster the
/// foreground. Every region is painted black.
#[derive(Debug, Clone)]
pub struct BinaryFrontend {
    /// Discard clusters smaller than this many pixels.
    pub filter_speckle_area: usize,
    /// A pixel is foreground when its red channel is below this threshold.
    pub threshold: u8,
    /// Whether to connect clusters diagonally.
    pub diagonal: bool,
}

impl Default for BinaryFrontend {
    fn default() -> Self {
        Self {
            filter_speckle_area: 16,
            threshold: 128,
            diagonal: false,
        }
    }
}

impl Frontend for BinaryFrontend {
    fn segment(&self, img: &ColorImage) -> Result<Segmentation, Error> {
        if img.width == 0 || img.height == 0 {
            return Err(Error::EmptyImage);
        }

        let width = img.width;
        let height = img.height;
        let threshold = self.threshold;
        let binary = img.to_binary_image(|c| c.r < threshold);
        let clusters = binary.to_clusters(self.diagonal);

        let mut seg = Segmentation::new(width as u32, height as u32);
        let black = Color::new(0, 0, 0);
        for i in 0..clusters.len() {
            let cluster = clusters.get_cluster(i);
            if cluster.size() >= self.filter_speckle_area {
                let mask = RegionMask::new(
                    cluster.to_binary_image(),
                    PointI32 {
                        x: cluster.rect.left,
                        y: cluster.rect.top,
                    },
                );
                seg.layers.push(Layer {
                    paint: Paint::Solid(black),
                    mask,
                });
            }
        }

        Ok(seg)
    }
}

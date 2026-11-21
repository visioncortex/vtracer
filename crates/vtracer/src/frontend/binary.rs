use visioncortex::{BinaryImage, Color, ColorImage, PointI32};

use crate::error::Error;
use crate::ir::{Layer, Paint, RegionMask, Segmentation};

use super::Frontend;

/// Grayscale intensity (0..=255) used by every thresholding method.
#[inline]
fn intensity_rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32 + g as u32 + b as u32) / 3
}

/// How the binary frontend separates foreground (dark) from background pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Threshold {
    /// Global cutoff: a pixel is foreground when its intensity is below this
    /// value (0..=255). Fast and predictable; best for clean, evenly-lit input.
    Fixed(u8),
    /// Bradley–Roth adaptive threshold: a pixel is foreground when its
    /// intensity is more than `t` percent below the mean of the surrounding
    /// `window`×`window` block. Handles uneven lighting and shadows that defeat
    /// a single global cutoff. Computed in one pass with a summed-area table,
    /// so it stays O(pixels) regardless of window size.
    Adaptive {
        /// Window side length in pixels; `0` auto-derives ~1/8 of the shorter
        /// image dimension (the value suggested by the paper).
        window: u32,
        /// Sensitivity, as a percentage below the local mean (paper default 15).
        t: f64,
    },
}

impl Threshold {
    /// Bradley–Roth adaptive thresholding with the paper's defaults
    /// (auto window, `t = 15`).
    pub const fn adaptive() -> Self {
        Threshold::Adaptive {
            window: 0,
            t: 15.0,
        }
    }
}

impl Default for Threshold {
    fn default() -> Self {
        Threshold::Fixed(128)
    }
}

/// Binary (black/white) frontend: threshold the image then cluster the
/// foreground. Every region is painted black.
///
/// The [`Threshold`] chooses how foreground is separated from background — a
/// fixed global cutoff ([`Threshold::Fixed`]) or Bradley–Roth adaptive
/// thresholding ([`Threshold::Adaptive`]) for scans and photos with uneven
/// lighting.
///
/// Speckle removal drops clusters smaller than `min_area` px as the clusters
/// are collected, matching the pre-1.0 binary path (`cluster.size() >= area`).
#[derive(Debug, Clone)]
pub struct BinaryFrontend {
    /// How foreground pixels are selected (see [`Threshold`]).
    pub threshold: Threshold,
    /// Whether to connect clusters diagonally.
    pub diagonal: bool,
    /// Discard clusters smaller than this many pixels (0 = keep all).
    pub min_area: usize,
}

impl Default for BinaryFrontend {
    fn default() -> Self {
        Self {
            threshold: Threshold::default(),
            diagonal: false,
            min_area: 0,
        }
    }
}

impl BinaryFrontend {
    /// Binarize `img` into a foreground mask according to [`Self::threshold`].
    fn binarize(&self, img: &ColorImage) -> BinaryImage {
        match self.threshold {
            Threshold::Fixed(value) => {
                let (w, h) = (img.width, img.height);
                let value = value as u32;
                let raw = &img.pixels;
                let mut out = BinaryImage::new_w_h(w, h);
                for i in 0..w * h {
                    let b = i * 4;
                    if intensity_rgb(raw[b], raw[b + 1], raw[b + 2]) < value {
                        out.set_pixel_index(i, true);
                    }
                }
                out
            }
            Threshold::Adaptive { window, t } => adaptive_bradley_roth(img, window, t),
        }
    }
}

/// Bradley–Roth adaptive thresholding via a zero-padded `(w+1)×(h+1)`
/// summed-area table (`u64` to prevent overflow on >16 MP images).
///
/// For each pixel, compare its intensity to the mean of a surrounding window:
/// it is foreground when `value <= mean * (1 - t/100)`, i.e. more than `t`
/// percent darker than its neighborhood.
fn adaptive_bradley_roth(img: &ColorImage, window: u32, t: f64) -> BinaryImage {
    let (w, h) = (img.width, img.height);
    let raw = &img.pixels;
    let sw = w + 1;
    let mut sat = vec![0u64; sw * (h + 1)];

    // Build zero-padded SAT in one branchless pass over `raw`.
    for y in 0..h {
        let mut row_sum = 0u64;
        let prev_row = y * sw;
        let cur_row = (y + 1) * sw;
        let px_row = y * w;
        for x in 0..w {
            let b = (px_row + x) * 4;
            let v = intensity_rgb(raw[b], raw[b + 1], raw[b + 2]) as u64;
            row_sum += v;
            sat[cur_row + x + 1] = sat[prev_row + x + 1] + row_sum;
        }
    }

    // Window: 0 => auto (~1/8 of the shorter side, per the paper), min 1.
    let side = if window == 0 {
        (w.min(h) / 8).max(1)
    } else {
        window as usize
    };
    let half = side / 2;
    let factor = 1.0 - t.clamp(0.0, 100.0) / 100.0;

    let mut out = BinaryImage::new_w_h(w, h);
    for y in 0..h {
        let y0 = y.saturating_sub(half);
        let y1 = (y + half).min(h - 1) + 1;
        let r0 = y0 * sw;
        let r1 = y1 * sw;
        let dy = y1 - y0;
        let px_row = y * w;
        for x in 0..w {
            let x0 = x.saturating_sub(half);
            let x1 = (x + half).min(w - 1) + 1;

            let count = ((x1 - x0) * dy) as f64;
            let sum = (sat[r1 + x1] + sat[r0 + x0] - sat[r0 + x1] - sat[r1 + x0]) as f64;
            let b = (px_row + x) * 4;
            let value = intensity_rgb(raw[b], raw[b + 1], raw[b + 2]) as f64;

            // value <= mean * factor  ⇔  value * count <= sum * factor
            if value * count <= sum * factor {
                out.set_pixel_index(px_row + x, true);
            }
        }
    }
    out
}

impl Frontend for BinaryFrontend {
    fn segment(&self, img: &ColorImage) -> Result<Segmentation, Error> {
        if img.width == 0 || img.height == 0 {
            return Err(Error::EmptyImage);
        }

        let width = img.width;
        let height = img.height;
        let binary = self.binarize(img);
        let clusters = binary.to_clusters(self.diagonal);

        let mut seg = Segmentation::new(width as u32, height as u32);
        let black = Color::new(0, 0, 0);
        for i in 0..clusters.len() {
            let cluster = clusters.get_cluster(i);
            if cluster.size() < self.min_area {
                continue;
            }
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

        Ok(seg)
    }
}

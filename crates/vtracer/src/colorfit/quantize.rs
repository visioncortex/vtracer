use visioncortex::Color;

use crate::ir::{Paint, Segmentation};

use super::oklab::Oklab;
use super::ColorFitter;

/// Reduce the layer palette to at most `max_colors` representative colors via
/// area-weighted median cut, then snap each layer to the nearest representative
/// (in OKLab).
#[derive(Debug, Clone)]
pub struct AutoQuantize {
    pub max_colors: usize,
}

impl Default for AutoQuantize {
    fn default() -> Self {
        Self { max_colors: 16 }
    }
}

#[derive(Clone, Copy)]
struct Sample {
    color: Color,
    weight: u64,
}

struct Bucket {
    samples: Vec<Sample>,
}

impl Bucket {
    /// Extent (max - min) of the given channel across the bucket.
    fn channel_range(&self, channel: usize) -> u8 {
        let mut lo = u8::MAX;
        let mut hi = u8::MIN;
        for s in &self.samples {
            let v = s.color.rgb_u8()[channel];
            lo = lo.min(v);
            hi = hi.max(v);
        }
        hi.saturating_sub(lo)
    }

    fn widest_channel(&self) -> usize {
        let mut best = 0;
        let mut best_range = 0u8;
        for c in 0..3 {
            let r = self.channel_range(c);
            if r > best_range {
                best_range = r;
                best = c;
            }
        }
        best
    }

    fn total_weight(&self) -> u64 {
        self.samples.iter().map(|s| s.weight).sum()
    }

    /// Weighted-average representative color.
    fn representative(&self) -> Color {
        let mut r = 0u64;
        let mut g = 0u64;
        let mut b = 0u64;
        let mut w = 0u64;
        for s in &self.samples {
            let rgb = s.color.rgb_u8();
            r += rgb[0] as u64 * s.weight;
            g += rgb[1] as u64 * s.weight;
            b += rgb[2] as u64 * s.weight;
            w += s.weight;
        }
        if w == 0 {
            return Color::new(0, 0, 0);
        }
        Color::new((r / w) as u8, (g / w) as u8, (b / w) as u8)
    }

    /// Split at the weighted median of the widest channel.
    fn split(mut self) -> (Bucket, Bucket) {
        let channel = self.widest_channel();
        self.samples
            .sort_by_key(|s| s.color.rgb_u8()[channel]);
        let half = self.total_weight() / 2;
        let mut acc = 0u64;
        let mut cut = 1;
        for (i, s) in self.samples.iter().enumerate() {
            acc += s.weight;
            if acc >= half {
                cut = (i + 1).clamp(1, self.samples.len().saturating_sub(1).max(1));
                break;
            }
        }
        let right = self.samples.split_off(cut);
        (Bucket { samples: self.samples }, Bucket { samples: right })
    }
}

impl ColorFitter for AutoQuantize {
    fn fit(&self, seg: &mut Segmentation) {
        if self.max_colors == 0 || seg.layers.is_empty() {
            return;
        }

        let samples: Vec<Sample> = seg
            .layers
            .iter()
            .map(|l| Sample {
                color: l.paint.color(),
                weight: l.mask.area() as u64 + 1,
            })
            .collect();

        let mut buckets = vec![Bucket { samples }];
        while buckets.len() < self.max_colors {
            // Split the bucket with the widest single-channel range.
            let target = buckets
                .iter()
                .enumerate()
                .filter(|(_, b)| b.samples.len() > 1)
                .max_by_key(|(_, b)| b.channel_range(b.widest_channel()));
            let Some((idx, _)) = target else { break };
            let bucket = buckets.swap_remove(idx);
            let (a, b) = bucket.split();
            buckets.push(a);
            buckets.push(b);
        }

        let palette: Vec<Color> = buckets.iter().map(Bucket::representative).collect();
        let lab: Vec<Oklab> = palette.iter().map(Oklab::from_color).collect();

        for layer in &mut seg.layers {
            let target = Oklab::from_color(&layer.paint.color());
            let mut best = palette[0];
            let mut best_dist = f64::INFINITY;
            for (i, entry) in palette.iter().enumerate() {
                let d = target.distance_squared(&lab[i]);
                if d < best_dist {
                    best_dist = d;
                    best = *entry;
                }
            }
            layer.paint = Paint::Solid(best);
        }
    }
}

use crate::ir::{Layer, Segmentation};

use super::ColorFitter;

/// Union consecutive layers that share a paint into a single layer. Run this
/// after palette snapping (which is what creates runs of identical paints) to
/// cut the shape count without changing appearance.
#[derive(Debug, Clone, Default)]
pub struct MergeAdjacent;

impl ColorFitter for MergeAdjacent {
    fn fit(&self, seg: &mut Segmentation) {
        if seg.layers.len() < 2 {
            return;
        }
        let mut merged: Vec<Layer> = Vec::with_capacity(seg.layers.len());
        for layer in seg.layers.drain(..) {
            if let Some(last) = merged.last_mut() {
                if last.paint == layer.paint {
                    last.mask = last.mask.union(&layer.mask);
                    continue;
                }
            }
            merged.push(layer);
        }
        seg.layers = merged;
    }
}

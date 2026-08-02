use crate::ir::{Layer, RegionMask, Segmentation};

use super::ColorFitter;

/// Union consecutive layers that share a paint into a single layer. Run this
/// after palette snapping (which is what creates runs of identical paints) to
/// cut the shape count without changing appearance.
#[derive(Debug, Clone, Default)]
pub struct MergeAdjacent;

/// Collapse one run of same-paint layers and push the result.
///
/// The whole run is unioned in a single pass — folding pairwise would reallocate
/// and rewrite a canvas-sized accumulator once per layer. See
/// [`RegionMask::union_all`].
fn flush(run: &mut Vec<Layer>, out: &mut Vec<Layer>) {
    match run.len() {
        0 => {}
        1 => out.push(run.pop().expect("run is non-empty")),
        _ => {
            let paint = run[0].paint;
            let masks: Vec<&RegionMask> = run.iter().map(|l| &l.mask).collect();
            let mask = RegionMask::union_all(&masks);
            out.push(Layer { paint, mask });
            run.clear();
        }
    }
}

impl ColorFitter for MergeAdjacent {
    fn fit(&self, seg: &mut Segmentation) {
        if seg.layers.len() < 2 {
            return;
        }
        let mut merged: Vec<Layer> = Vec::with_capacity(seg.layers.len());
        let mut run: Vec<Layer> = Vec::new();
        for layer in seg.layers.drain(..) {
            if run.first().is_some_and(|first| first.paint != layer.paint) {
                flush(&mut run, &mut merged);
            }
            run.push(layer);
        }
        flush(&mut run, &mut merged);
        seg.layers = merged;
    }
}

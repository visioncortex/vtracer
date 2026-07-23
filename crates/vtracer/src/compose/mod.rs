//! Compositing: turn a [`Segmentation`] into a [`VectorDoc`].
//!
//! Only **stacked** composition is implemented: each layer is traced
//! independently into closed outlines and stacked in paint order (painter's
//! algorithm). The **mosaic** compositor — gapless tessellation with shared
//! boundary geometry — is a separate milestone and not built yet.

use crate::fitter::CurveFitter;
use crate::ir::{Segmentation, Shape, VectorDoc};

/// Which compositing strategy the pipeline uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compositing {
    /// Independent per-region closed outlines, stacked bottom-to-top.
    Stacked,
}

/// Trace every layer's closed outline and stack the shapes in paint order.
pub fn compose_stacked(seg: &Segmentation, fitter: &dyn CurveFitter) -> VectorDoc {
    let mut doc = VectorDoc::new(seg.width, seg.height);
    for layer in &seg.layers {
        let path = fitter.fit_region(&layer.mask);
        if !path.is_empty() {
            doc.shapes.push(Shape {
                paint: layer.paint,
                path,
            });
        }
    }
    doc
}

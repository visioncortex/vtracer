//! Compositing: turn a [`Segmentation`] into a [`VectorDoc`].
//!
//! * **Stacked** — each layer is traced independently into closed outlines and
//!   stacked in paint order (painter's algorithm).
//! * **Mosaic** — a seam-free gapless tessellation with shared boundary
//!   geometry (see [`crate::mosaic`]).

use crate::error::Error;
use crate::fitter::CurveFitter;
use crate::ir::{Segmentation, Shape, VectorDoc};
use crate::mosaic::{compose_mosaic, SegmentFitter};
use crate::progress::{Ctx, Phase};

/// Which compositing strategy the pipeline uses. Each variant owns its fitter.
pub enum Compositing {
    /// Independent per-region closed outlines, stacked bottom-to-top.
    Stacked(Box<dyn CurveFitter>),
    /// Seam-free gapless tessellation via a shared boundary graph.
    Mosaic {
        fitter: Box<dyn SegmentFitter>,
        /// Merge flattened neighbours whose colors are within this diff —
        /// rejoins regions the stacked gradient layering had split. Usually
        /// the clustering gradient step; `0` disables merging.
        merge_diff: i32,
    },
}

impl Compositing {
    /// Run the selected compositor over a segmentation.
    pub fn compose(&self, seg: &Segmentation) -> VectorDoc {
        match self {
            Compositing::Stacked(fitter) => compose_stacked(seg, fitter.as_ref()),
            Compositing::Mosaic { fitter, merge_diff } => {
                compose_mosaic(seg, fitter.as_ref(), *merge_diff)
            }
        }
    }

    /// Progress- and cancellation-aware compositing.
    ///
    /// Stacked mode reports per-layer progress and can be cancelled between
    /// layers. Mosaic builds its boundary graph in one pass, so it reports
    /// coarsely (start/end) and is cancellable only at the boundaries — the
    /// dominant cost is upstream in clustering, which cancels finely.
    pub fn compose_with(&self, seg: &Segmentation, ctx: &mut Ctx) -> Result<VectorDoc, Error> {
        match self {
            Compositing::Stacked(fitter) => compose_stacked_with(seg, fitter.as_ref(), ctx),
            Compositing::Mosaic { fitter, merge_diff } => {
                ctx.check()?;
                ctx.report(Phase::Compose, 0.0);
                let doc = compose_mosaic(seg, fitter.as_ref(), *merge_diff);
                ctx.check()?;
                ctx.report(Phase::Compose, 1.0);
                Ok(doc)
            }
        }
    }
}

/// Progress-aware [`compose_stacked`]: reports after each layer and checks for
/// cancellation between them.
fn compose_stacked_with(
    seg: &Segmentation,
    fitter: &dyn CurveFitter,
    ctx: &mut Ctx,
) -> Result<VectorDoc, Error> {
    let mut doc = VectorDoc::new(seg.width, seg.height);
    let total = seg.layers.len().max(1);
    for (i, layer) in seg.layers.iter().enumerate() {
        ctx.check()?;
        let path = fitter.fit_region(&layer.mask);
        if !path.is_empty() {
            doc.shapes.push(Shape {
                paint: layer.paint,
                path,
            });
        }
        ctx.report(Phase::Compose, (i + 1) as f32 / total as f32);
    }
    Ok(doc)
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

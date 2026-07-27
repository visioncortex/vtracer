//! Compositing: turn a [`Segmentation`] into a [`VectorDoc`].
//!
//! * **Stacked** — each layer is traced independently into closed outlines and
//!   stacked in paint order (painter's algorithm).
//! * **Mosaic** — a seam-free gapless tessellation with shared boundary
//!   geometry (see [`crate::mosaic`]).
//!
//! Both compositors run the pipeline's [`CurvePass`]es over every fitted
//! contour before assembling paths — geometry passes have to happen here, on
//! the fitted geometry, so that in mosaic mode each shared boundary segment
//! is transformed exactly once for both of its faces.

use crate::error::Error;
use crate::fitter::CurveFitter;
use crate::ir::{MultiPath, RegionMask, Segmentation, Shape, VectorDoc};
use crate::mosaic::{compose_mosaic, SegmentFitter};
use crate::progress::{Ctx, Phase};
use crate::simplify::CurvePass;

/// Which compositing strategy the pipeline uses. Each variant owns its fitter.
pub enum Compositing {
    /// Independent per-region closed outlines, stacked bottom-to-top.
    Stacked(Box<dyn CurveFitter>),
    /// Seam-free gapless tessellation via a shared boundary graph.
    Mosaic {
        fitter: Box<dyn SegmentFitter>,
        /// Merge flattened neighbours whose colors are within this diff —
        /// rejoins regions the stacked gradient layering had split. Usually
        /// the clustering gradient step; `0` still merges identical-color
        /// neighbours, negative disables merging entirely.
        merge_diff: i32,
    },
}

impl Compositing {
    /// Run the selected compositor over a segmentation, applying `passes` to
    /// every fitted contour before paths are assembled.
    pub fn compose(&self, seg: &Segmentation, passes: &[Box<dyn CurvePass>]) -> VectorDoc {
        match self {
            Compositing::Stacked(fitter) => compose_stacked(seg, fitter.as_ref(), passes),
            Compositing::Mosaic { fitter, merge_diff } => {
                compose_mosaic(seg, fitter.as_ref(), *merge_diff, passes)
            }
        }
    }

    /// Progress- and cancellation-aware compositing.
    ///
    /// Stacked mode reports per-layer progress and can be cancelled between
    /// layers. Mosaic builds its boundary graph in one pass, so it reports
    /// coarsely (start/end) and is cancellable only at the boundaries — the
    /// dominant cost is upstream in clustering, which cancels finely.
    pub fn compose_with(
        &self,
        seg: &Segmentation,
        passes: &[Box<dyn CurvePass>],
        ctx: &mut Ctx,
    ) -> Result<VectorDoc, Error> {
        match self {
            Compositing::Stacked(fitter) => compose_stacked_with(seg, fitter.as_ref(), passes, ctx),
            Compositing::Mosaic { fitter, merge_diff } => {
                ctx.check()?;
                ctx.report(Phase::Compose, 0.0);
                let doc = compose_mosaic(seg, fitter.as_ref(), *merge_diff, passes);
                ctx.check()?;
                ctx.report(Phase::Compose, 1.0);
                Ok(doc)
            }
        }
    }
}

/// Fit one region's outlines and run the curve passes over each contour.
/// Stacked contours are closed rings, so the ring form of each pass applies.
fn fit_region(
    fitter: &dyn CurveFitter,
    mask: &RegionMask,
    passes: &[Box<dyn CurvePass>],
) -> MultiPath {
    let mut path = MultiPath::new();
    for mut geom in fitter.fit_region(mask) {
        for pass in passes {
            geom = pass.ring(geom);
        }
        path.push(geom.into_closed_subpath());
    }
    path
}

/// Progress-aware [`compose_stacked`]: reports after each layer and checks for
/// cancellation between them.
fn compose_stacked_with(
    seg: &Segmentation,
    fitter: &dyn CurveFitter,
    passes: &[Box<dyn CurvePass>],
    ctx: &mut Ctx,
) -> Result<VectorDoc, Error> {
    let mut doc = VectorDoc::new(seg.width, seg.height);
    let total = seg.layers.len().max(1);
    for (i, layer) in seg.layers.iter().enumerate() {
        ctx.check()?;
        let path = fit_region(fitter, &layer.mask, passes);
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
pub fn compose_stacked(
    seg: &Segmentation,
    fitter: &dyn CurveFitter,
    passes: &[Box<dyn CurvePass>],
) -> VectorDoc {
    let mut doc = VectorDoc::new(seg.width, seg.height);
    for layer in &seg.layers {
        let path = fit_region(fitter, &layer.mask, passes);
        if !path.is_empty() {
            doc.shapes.push(Shape {
                paint: layer.paint,
                path,
            });
        }
    }
    doc
}

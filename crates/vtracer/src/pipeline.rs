//! The pipeline driver: composes the stages and runs an image through them.

use visioncortex::ColorImage;

use crate::colorfit::ColorFitter;
use crate::compose::Compositing;
use crate::error::Error;
use crate::frontend::Frontend;
use crate::ir::{Segmentation, VectorDoc};
use crate::optimize::OptimizerPass;
use crate::progress::{CancelToken, Ctx, Phase, Progress};
use crate::svg::SvgWriter;

/// A fully-assembled vectorization pipeline. Build one with
/// [`crate::Config::build`], or construct it directly for full control.
pub struct Pipeline {
    pub frontend: Box<dyn Frontend>,
    pub color_fitters: Vec<Box<dyn ColorFitter>>,
    pub compositing: Compositing,
    pub optimizers: Vec<Box<dyn OptimizerPass>>,
    pub writer: SvgWriter,
}

impl Pipeline {
    /// Run the pipeline to the output document IR (before serialization).
    ///
    /// Equivalent to [`run_with_progress`](Pipeline::run_with_progress) with a
    /// fresh (never-cancelled) token and a no-op progress callback.
    pub fn run(&self, img: &ColorImage) -> Result<VectorDoc, Error> {
        self.run_with_progress(img, &CancelToken::new(), &mut |_| {})
    }

    /// Run the pipeline, publishing [`Progress`] updates and honoring the
    /// [`CancelToken`].
    ///
    /// Intended to be called on a worker thread: hand a clone of `cancel` to
    /// the UI so a button can abort, and forward `on_progress` to a channel
    /// that drives a progress bar. Returns [`Error::Cancelled`] if the token is
    /// tripped. See [`crate::progress`] for a usage example.
    pub fn run_with_progress(
        &self,
        img: &ColorImage,
        cancel: &CancelToken,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<VectorDoc, Error> {
        let mut ctx = Ctx::new(cancel, on_progress);
        let seg = self.frontend.segment_with(img, &mut ctx)?;
        // `seg` is owned and about to be consumed, so no clone is needed here.
        self.finish_ctx(seg, &mut ctx)
    }

    /// Phase 1 of 2 — run **only** the frontend (the expensive clustering step)
    /// and return a reusable [`Segmentation`].
    ///
    /// Cache the result and feed it to [`finish`](Pipeline::finish) to
    /// re-render with different color-fitting, curve-fitting, or optimization
    /// parameters *without repaying the clustering cost* — the core of an
    /// interactive tuning loop. Only re-run `segment` when a parameter that
    /// affects clustering itself changes (color precision, layer difference,
    /// speckle filter, binary threshold, the frontend choice).
    pub fn segment(&self, img: &ColorImage) -> Result<Segmentation, Error> {
        self.segment_with_progress(img, &CancelToken::new(), &mut |_| {})
    }

    /// [`segment`](Pipeline::segment) with progress reporting and cancellation.
    pub fn segment_with_progress(
        &self,
        img: &ColorImage,
        cancel: &CancelToken,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<Segmentation, Error> {
        let mut ctx = Ctx::new(cancel, on_progress);
        self.frontend.segment_with(img, &mut ctx)
    }

    /// Phase 2 of 2 — color fitting → compositing → optimization, reusing a
    /// [`Segmentation`] produced by [`segment`](Pipeline::segment).
    ///
    /// The segmentation is cloned internally (color fitting mutates it), so the
    /// cached copy stays pristine and can be reused across many `finish` calls
    /// with different pipelines. The frontend of `self` is not used here; build
    /// the tuning pipeline with the color/curve/optimize parameters you want
    /// and the *same* clustering parameters that produced `seg`.
    pub fn finish(&self, seg: &Segmentation) -> Result<VectorDoc, Error> {
        self.finish_with_progress(seg, &CancelToken::new(), &mut |_| {})
    }

    /// [`finish`](Pipeline::finish) with progress reporting and cancellation.
    /// Progress starts at the [`Phase::Compose`] stage (segmentation is skipped).
    pub fn finish_with_progress(
        &self,
        seg: &Segmentation,
        cancel: &CancelToken,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<VectorDoc, Error> {
        let mut ctx = Ctx::new(cancel, on_progress);
        self.finish_ctx(seg.clone(), &mut ctx)
    }

    /// Downstream stages (color fit → compose → optimize) over an owned
    /// segmentation. Shared by the one-shot and two-phase entry points; takes
    /// ownership so the one-shot path avoids a clone.
    fn finish_ctx(&self, mut seg: Segmentation, ctx: &mut Ctx) -> Result<VectorDoc, Error> {
        for fitter in &self.color_fitters {
            fitter.fit(&mut seg);
            ctx.check()?;
        }

        let mut doc = self.compositing.compose_with(&seg, ctx)?;

        let total = self.optimizers.len().max(1);
        for (i, pass) in self.optimizers.iter().enumerate() {
            ctx.check()?;
            pass.run(&mut doc);
            ctx.report(Phase::Optimize, (i + 1) as f32 / total as f32);
        }
        // Always emit a terminal 100% so a UI can settle even with no passes.
        ctx.report(Phase::Optimize, 1.0);

        Ok(doc)
    }

    /// Run the pipeline and serialize the result to an SVG string.
    pub fn to_svg(&self, img: &ColorImage) -> Result<String, Error> {
        Ok(self.writer.write(&self.run(img)?))
    }
}

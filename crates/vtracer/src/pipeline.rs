//! The pipeline driver: composes the stages and runs an image through them.

use visioncortex::ColorImage;

use crate::colorfit::ColorFitter;
use crate::compose::Compositing;
use crate::error::Error;
use crate::frontend::Frontend;
use crate::ir::VectorDoc;
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

        let mut seg = self.frontend.segment_with(img, &mut ctx)?;

        for fitter in &self.color_fitters {
            fitter.fit(&mut seg);
            ctx.check()?;
        }

        let mut doc = self.compositing.compose_with(&seg, &mut ctx)?;

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

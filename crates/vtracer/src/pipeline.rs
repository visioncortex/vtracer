//! The pipeline driver: composes the stages and runs an image through them.

use visioncortex::ColorImage;

use crate::colorfit::ColorFitter;
use crate::compose::Compositing;
use crate::error::Error;
use crate::frontend::Frontend;
use crate::ir::VectorDoc;
use crate::optimize::OptimizerPass;
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
    pub fn run(&self, img: &ColorImage) -> Result<VectorDoc, Error> {
        let mut seg = self.frontend.segment(img)?;

        for fitter in &self.color_fitters {
            fitter.fit(&mut seg);
        }

        let mut doc = self.compositing.compose(&seg);

        for pass in &self.optimizers {
            pass.run(&mut doc);
        }

        Ok(doc)
    }

    /// Run the pipeline and serialize the result to an SVG string.
    pub fn to_svg(&self, img: &ColorImage) -> Result<String, Error> {
        Ok(self.writer.write(&self.run(img)?))
    }
}

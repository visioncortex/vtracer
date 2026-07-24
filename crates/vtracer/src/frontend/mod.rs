//! Frontends: algorithms that turn a raster image into a [`Segmentation`].
//!
//! Built-ins:
//! * [`ColorClusterFrontend`] — hierarchical color clustering (the classic
//!   VTracer color path), including transparency keying.
//! * [`BinaryFrontend`] — threshold to black/white then cluster.
//!
//! Third parties can implement [`Frontend`] to feed external label maps or ML
//! segmentation into the pipeline.

mod binary;
mod color_cluster;
mod keying;

pub use binary::{BinaryFrontend, Threshold};
pub use color_cluster::ColorClusterFrontend;

use visioncortex::ColorImage;

use crate::error::Error;
use crate::ir::Segmentation;
use crate::progress::Ctx;

/// A frontend segments a raster image into ordered paint layers.
pub trait Frontend {
    fn segment(&self, img: &ColorImage) -> Result<Segmentation, Error>;

    /// Progress- and cancellation-aware segmentation.
    ///
    /// The default runs [`segment`](Frontend::segment) and then honors
    /// cancellation (coarse: one report at completion, cancel observed after
    /// the whole segmentation). Frontends that can step incrementally — like
    /// [`ColorClusterFrontend`] — override this to report fine-grained
    /// progress and observe cancellation between batches.
    fn segment_with(&self, img: &ColorImage, ctx: &mut Ctx) -> Result<Segmentation, Error> {
        let seg = self.segment(img)?;
        ctx.check()?;
        ctx.report(crate::progress::Phase::Segment, 1.0);
        Ok(seg)
    }
}

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

pub use binary::BinaryFrontend;
pub use color_cluster::ColorClusterFrontend;

use visioncortex::ColorImage;

use crate::error::Error;
use crate::ir::Segmentation;

/// A frontend segments a raster image into ordered paint layers.
pub trait Frontend {
    fn segment(&self, img: &ColorImage) -> Result<Segmentation, Error>;
}

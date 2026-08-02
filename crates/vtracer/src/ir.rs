//! Core intermediate representation shared by the pipeline stages.
//!
//! Two IRs flow through the pipeline:
//!
//! * [`Segmentation`] — the frontend output: ordered paint layers over a
//!   raster canvas (painter's algorithm, bottom to top). This is what the
//!   [`crate::colorfit`] stages rewrite.
//! * [`VectorDoc`] — the output document: resolved shapes with fitted paths.
//!   This is what the [`crate::optimize`] passes and the [`crate::svg`] writer
//!   operate on.

mod region;
mod vector;

pub use region::{Layer, RegionMask, Segmentation};
pub use vector::{MultiPath, PathCmd, Shape, SubPath, VectorDoc};

use visioncortex::Color;

/// The final appearance of a region. Only solid colors are supported today;
/// the enum leaves room for gradients and patterns later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    Solid(Color),
}

impl Paint {
    /// The representative solid color of this paint.
    pub fn color(&self) -> Color {
        match self {
            Paint::Solid(c) => *c,
        }
    }
}

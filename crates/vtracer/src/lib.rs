//! # vtracer
//!
//! A vectorization *framework*: raster images become vector graphics through a
//! pipeline of pluggable stages.
//!
//! ```text
//! Frontend ─▶ ColorFitter* ─▶ Compositing ─▶ CurveFitter ─▶ VectorDoc
//!                                                              │
//!                                          OptimizerPass* ─────┤
//!                                                              ▼
//!                                                          SvgWriter ─▶ SVG
//! ```
//!
//! The crate is wasm-safe: it performs no file or image I/O (that lives in the
//! `vtracer-cli` wrapper). Everything here compiles to
//! `wasm32-unknown-unknown`.
//!
//! ## Quick start
//!
//! ```no_run
//! use vtracer::{Config, ColorImage};
//!
//! # fn load() -> ColorImage { todo!() }
//! let img: ColorImage = load();
//! let svg = Config::default().build().unwrap().to_svg(&img).unwrap();
//! ```
//!
//! For finer control, assemble a [`Pipeline`] directly from the stage traits
//! in [`frontend`], [`colorfit`], [`fitter`], [`compose`], [`optimize`], and
//! [`svg`].

pub mod colorfit;
pub mod compose;
pub mod config;
pub mod error;
pub mod fitter;
pub mod frontend;
pub mod ir;
pub mod mosaic;
pub mod optimize;
pub mod pipeline;
pub mod progress;
pub mod svg;

pub use config::{ColorMode, Config, FitMode, Hierarchical, Preset};
pub use error::Error;
pub use pipeline::Pipeline;
pub use progress::{CancelToken, Phase, Progress};

// Re-export the visioncortex value types callers need at the boundary.
pub use visioncortex::{Color, ColorImage, PointF64, PointI32};

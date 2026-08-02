//! # vtracer
//!
//! Convert raster images into vector graphics (SVG). VTracer is a
//! *framework*: the conversion runs as a pipeline of small, swappable stages,
//! so you can reach for a one-line convenience call or rebuild the pipeline
//! stage by stage.
//!
//! ## The pipeline
//!
//! ```text
//! image ──▶ Frontend ──▶ ColorFitter ──▶ Compositing ──▶ Optimizer ──▶ SvgWriter ──▶ SVG
//! ```
//!
//! Two intermediate representations carry the work between stages:
//!
//! * a [`Segmentation`] — the frontend's output: flat-color regions (paint
//!   layers) over the canvas, which the color fitters may recolor or quantize;
//! * a [`VectorDoc`] — the output document: resolved shapes with fitted vector
//!   paths, which the optimizer passes shrink and the writer serializes.
//!
//! Compositing is where the geometry is built — it runs the curve
//! [`CurveFitter`](fitter::CurveFitter) on each region outline, then any
//! geometry [`CurvePass`](simplify::CurvePass)es (curve simplification and the
//! like) over the fitted curves. Every stage is a trait in its own module, and
//! most can run more than once (a chain of color fitters, several optimizer
//! passes):
//!
//! | Stage | Trait | Module |
//! |---|---|---|
//! | Region forming | [`Frontend`](frontend::Frontend) | [`frontend`] |
//! | Recolor / quantize | [`ColorFitter`](colorfit::ColorFitter) | [`colorfit`] |
//! | Curve fitting | [`CurveFitter`](fitter::CurveFitter) | [`fitter`] |
//! | Geometry passes | [`CurvePass`](simplify::CurvePass) | [`simplify`] |
//! | Compositing | *(stacked or mosaic)* | [`compose`], [`mosaic`] |
//! | Optimization | [`OptimizerPass`](optimize::OptimizerPass) | [`optimize`] |
//! | Serialization | *(SVG writer)* | [`svg`] |
//!
//! ## Quick start
//!
//! [`Config`] is the high-level entry point: choose options (or start from a
//! [`Preset`]), [`build`](Config::build) a [`Pipeline`], and run it. The crate
//! does no image decoding — hand it a decoded [`ColorImage`] and get back an
//! SVG string.
//!
//! ```no_run
//! use vtracer::{Config, ColorImage, Preset};
//! # fn load() -> ColorImage { todo!() }
//! let img: ColorImage = load();
//!
//! // one-shot: image → SVG string, all defaults
//! let svg = Config::default().build()?.to_svg(&img)?;
//!
//! // start from a preset and tweak
//! let mut cfg = Config::from_preset(Preset::Poster);
//! cfg.simplify = Some(1.5); // paper.js-style curve simplification
//! let svg = cfg.build()?.to_svg(&img)?;
//! # Ok::<(), vtracer::Error>(())
//! ```
//!
//! ## Interactive tuning
//!
//! Segmentation is the expensive stage. A [`Session`] caches it and re-renders
//! only the cheap downstream stages when a non-clustering parameter changes,
//! re-segmenting automatically when it must — ideal behind a live UI with
//! sliders. See the [`session`] module.
//!
//! ## Extending the pipeline
//!
//! Build a [`Pipeline`] by hand to mix in your own stages: implement
//! [`Frontend`](frontend::Frontend) to feed an external label map or ML
//! segmentation, [`ColorFitter`](colorfit::ColorFitter) for a custom palette
//! policy, or [`CurvePass`](simplify::CurvePass) for a geometry transform.
//!
//! ## No I/O, wasm-safe
//!
//! Because it performs no file or image I/O, the crate compiles cleanly to
//! `wasm32-unknown-unknown`. Decoding and file handling live in the wrappers:
//! the `vtracer-cli` command-line tool, the `vtracer` Python package, and the
//! `@visioncortex/vtracer` Node package.

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
pub mod session;
pub mod simplify;
pub mod svg;

pub use config::{Clustering, Config, FitMode, Hierarchical, Preset, SegmentKey};
pub use error::Error;
pub use frontend::Threshold;
pub use ir::{Segmentation, VectorDoc};
pub use pipeline::Pipeline;
pub use progress::{CancelToken, Phase, Progress};
pub use session::Session;

// Re-export the visioncortex value types callers need at the boundary.
pub use visioncortex::{Color, ColorImage, PointF64, PointI32};

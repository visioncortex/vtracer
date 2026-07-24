//! Progress reporting and cancellation for long-running conversions.
//!
//! [`crate::Pipeline::run_with_progress`] takes a [`CancelToken`] and a
//! progress callback. On native targets, run it on a worker thread: the
//! callback publishes [`Progress`] to the UI and the token lets the UI abort
//! between work batches (clustering checks once per batch, so cancellation is
//! near-instant). The pipeline returns [`Error::Cancelled`] when the token is
//! tripped.
//!
//! There is deliberately no cooperative `tick()` here: that only existed in the
//! old browser build because the main thread could not block. The same API
//! works unchanged from a Web Worker.
//!
//! ```no_run
//! use vtracer::{Config, ColorImage};
//! use vtracer::progress::{CancelToken, Progress};
//!
//! # fn load() -> ColorImage { todo!() }
//! let pipeline = Config::default().build().unwrap();
//! let cancel = CancelToken::new();
//! # let img: ColorImage = load();
//! // hand `cancel.clone()` to the UI so a button can call `cancel.cancel()`
//! let mut on_progress = |p: Progress| eprintln!("{:?} {:.0}%", p.phase, p.fraction * 100.0);
//! let doc = pipeline.run_with_progress(&img, &cancel, &mut on_progress);
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::Error;

/// A cheaply-clonable cancellation flag shared between the UI and the worker.
///
/// Clone it, hand one copy to the worker thread running the pipeline and keep
/// the other; call [`cancel`](CancelToken::cancel) from any thread to request
/// an early stop. Clones share the same underlying flag.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A fresh, un-cancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation. Idempotent; safe to call from any thread.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Which pipeline phase a [`Progress`] update belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Frontend segmentation (color clustering) — usually the dominant cost.
    Segment,
    /// Compositing the segmentation into shapes.
    Compose,
    /// Output optimization passes.
    Optimize,
}

/// A progress update: the current [`Phase`] and how far through it we are.
///
/// `fraction` is *within* the phase, in `0.0..=1.0`. Clustering dominates
/// runtime, so a UI can weight the phases or simply show the phase label with
/// its fraction (e.g. "Clustering 45%").
#[derive(Clone, Copy, Debug)]
pub struct Progress {
    pub phase: Phase,
    pub fraction: f32,
}

/// Bundles the cancel token and progress sink threaded through the stages.
///
/// Stages call [`Ctx::check`] between batches to honor cancellation and
/// [`Ctx::report`] to publish progress.
pub struct Ctx<'a> {
    cancel: &'a CancelToken,
    on_progress: &'a mut dyn FnMut(Progress),
}

impl<'a> Ctx<'a> {
    /// Construct a context from a token and a progress callback.
    pub fn new(cancel: &'a CancelToken, on_progress: &'a mut dyn FnMut(Progress)) -> Self {
        Self {
            cancel,
            on_progress,
        }
    }

    /// Return [`Error::Cancelled`] if cancellation has been requested.
    pub fn check(&self) -> Result<(), Error> {
        if self.cancel.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Publish a progress update for `phase` at `fraction` (clamped to 0..=1).
    pub fn report(&mut self, phase: Phase, fraction: f32) {
        (self.on_progress)(Progress {
            phase,
            fraction: fraction.clamp(0.0, 1.0),
        });
    }
}

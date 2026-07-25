//! Interactive tuning session: cache the expensive clustering, re-render on the
//! cheap stages, and re-segment automatically only when it's actually needed.
//!
//! A desktop app loads an image once, then calls [`Session::render`] on every
//! slider change with a fresh [`Config`]. The session compares the config's
//! [`SegmentKey`](crate::config::SegmentKey) to what it last clustered and
//! re-segments only if a clustering parameter changed — the caller never has to
//! know which parameters those are.
//!
//! ```no_run
//! use vtracer::{Config, Session, ColorImage};
//! # fn load() -> ColorImage { todo!() }
//! let mut session = Session::new(load());
//!
//! // First render clusters the image.
//! let mut cfg = Config::default();
//! let _svg = session.render_svg(&cfg).unwrap();
//!
//! // Tuning a curve parameter reuses the cached segmentation (no re-cluster).
//! cfg.corner_threshold = 90;
//! let _svg = session.render_svg(&cfg).unwrap();
//!
//! // Changing a clustering parameter re-segments automatically.
//! cfg.filter_speckle = 8;
//! let _svg = session.render_svg(&cfg).unwrap();
//! ```

use visioncortex::ColorImage;

use crate::config::{Config, SegmentKey};
use crate::error::Error;
use crate::ir::{Segmentation, VectorDoc};
use crate::progress::{CancelToken, Progress};

/// A reusable converter for one image: clusters once, re-renders many times.
///
/// Build it with the source [`ColorImage`] and drive it with a [`Config`] per
/// render. The cached [`Segmentation`] is refreshed transparently whenever the
/// config's clustering parameters change.
pub struct Session {
    img: ColorImage,
    /// The segmentation and the key it was produced with (`None` until the
    /// first render).
    cache: Option<(SegmentKey, Segmentation)>,
}

impl Session {
    /// Start a session over `img`. Nothing is clustered until the first render.
    pub fn new(img: ColorImage) -> Self {
        Self { img, cache: None }
    }

    /// Whether the cached segmentation is missing or was clustered with
    /// different parameters than `key`.
    fn stale(&self, key: &SegmentKey) -> bool {
        self.cache.as_ref().map_or(true, |(k, _)| k != key)
    }

    /// The cached segmentation. Only call after ensuring the cache is fresh.
    fn segmentation(&self) -> &Segmentation {
        &self.cache.as_ref().expect("cache populated by caller").1
    }

    /// Render to the document IR, re-segmenting only if `cfg`'s clustering
    /// parameters differ from the cached segmentation's.
    pub fn render(&mut self, cfg: &Config) -> Result<VectorDoc, Error> {
        let pipeline = cfg.build()?;
        let key = cfg.segment_key();
        if self.stale(&key) {
            self.cache = Some((key, pipeline.segment(&self.img)?));
        }
        pipeline.finish(self.segmentation())
    }

    /// [`render`](Session::render), serialized to an SVG string.
    pub fn render_svg(&mut self, cfg: &Config) -> Result<String, Error> {
        let pipeline = cfg.build()?;
        let key = cfg.segment_key();
        if self.stale(&key) {
            self.cache = Some((key, pipeline.segment(&self.img)?));
        }
        Ok(pipeline.writer.write(&pipeline.finish(self.segmentation())?))
    }

    /// [`render`](Session::render) with progress reporting and cancellation.
    ///
    /// When a re-segmentation is needed, progress covers the [`Phase::Segment`]
    /// stage first, then the finish stages; on a cache hit only the finish
    /// stages report. Hand a clone of `cancel` to the UI to abort a long
    /// clustering pass.
    ///
    /// [`Phase::Segment`]: crate::Phase::Segment
    pub fn render_with_progress(
        &mut self,
        cfg: &Config,
        cancel: &CancelToken,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<VectorDoc, Error> {
        let pipeline = cfg.build()?;
        let key = cfg.segment_key();
        if self.stale(&key) {
            let seg = pipeline.segment_with_progress(&self.img, cancel, on_progress)?;
            self.cache = Some((key, seg));
        }
        pipeline.finish_with_progress(self.segmentation(), cancel, on_progress)
    }

    /// Drop the cached segmentation, forcing the next render to re-cluster.
    /// Use after replacing the source image out of band; normally unnecessary.
    pub fn invalidate(&mut self) {
        self.cache = None;
    }

    /// The source image this session renders.
    pub fn image(&self) -> &ColorImage {
        &self.img
    }
}

//! Python bindings for the `vtracer` vectorization framework.
//!
//! The API centers on a mutable [`Config`] object with named properties and
//! preset constructors, plus three input paths — a file, encoded image bytes,
//! or a raw RGBA buffer — each returning the SVG (or writing it to disk):
//!
//! ```python
//! import vtracer
//!
//! # one-liners
//! vtracer.convert_file("in.png", "out.svg")
//! svg = vtracer.convert_bytes(open("in.png", "rb").read())
//!
//! # rich, reusable config
//! cfg = vtracer.Config(mode="polygon", hierarchical="cutout")
//! cfg.max_colors = 8
//! cfg.palette = ["#1b1b1b", "#e0c088", "#5a7d3c"]
//! svg = cfg.convert_bytes(data)
//!
//! # presets
//! vtracer.Config.poster().convert_file("photo.jpg", "poster.svg")
//! ```

use std::io::Cursor;
use std::path::PathBuf;

use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;

use ::vtracer::{
    Color, ColorImage, Clustering, Config as CoreConfig, FitMode, Hierarchical, Preset,
};

// --- string <-> enum helpers -------------------------------------------------

fn parse<T: std::str::FromStr<Err = String>>(s: &str) -> PyResult<T> {
    s.parse().map_err(PyValueError::new_err)
}

fn clustering_str(c: Clustering) -> &'static str {
    match c {
        Clustering::ColorCluster => "color-cluster",
        Clustering::Binary => "bw",
        Clustering::Watershed => "watershed",
    }
}

fn hierarchical_str(h: Hierarchical) -> &'static str {
    match h {
        Hierarchical::Stacked => "stacked",
        Hierarchical::Cutout => "cutout",
    }
}

fn mode_str(m: FitMode) -> &'static str {
    match m {
        FitMode::Pixel => "pixel",
        FitMode::Polygon => "polygon",
        FitMode::Spline => "spline",
    }
}

fn parse_hex(token: &str) -> PyResult<Color> {
    let hex = token.strip_prefix('#').unwrap_or(token);
    if hex.len() != 6 {
        return Err(PyValueError::new_err(format!(
            "`{token}` is not a #rrggbb color"
        )));
    }
    let byte = |r: std::ops::Range<usize>| {
        u8::from_str_radix(&hex[r], 16)
            .map_err(|_| PyValueError::new_err(format!("`{token}` is not a #rrggbb color")))
    };
    Ok(Color::new(byte(0..2)?, byte(2..4)?, byte(4..6)?))
}

// --- image helpers -----------------------------------------------------------

fn dynimg_to_color(img: image::DynamicImage) -> ColorImage {
    let img = img.to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    ColorImage {
        pixels: img.into_raw(),
        width: w,
        height: h,
    }
}

fn decode_bytes(bytes: &[u8], format: Option<&str>) -> PyResult<ColorImage> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes));
    match format {
        Some(ext) => {
            let fmt = image::ImageFormat::from_extension(ext)
                .ok_or_else(|| PyValueError::new_err(format!("unknown image format `{ext}`")))?;
            reader.set_format(fmt);
        }
        None => {
            reader = reader
                .with_guessed_format()
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
        }
    }
    let img = reader
        .decode()
        .map_err(|e| PyValueError::new_err(format!("failed to decode image: {e}")))?;
    Ok(dynimg_to_color(img))
}

// --- Config ------------------------------------------------------------------

/// Conversion configuration. Construct with keyword arguments or a preset,
/// mutate via properties, then call one of the `convert_*` methods.
#[pyclass(name = "Config")]
#[derive(Clone)]
struct PyConfig {
    inner: CoreConfig,
}

impl PyConfig {
    fn to_svg(&self, img: &ColorImage) -> PyResult<String> {
        self.inner
            .build()
            .map_err(|e| PyValueError::new_err(e.to_string()))?
            .to_svg(img)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

#[pymethods]
impl PyConfig {
    #[new]
    #[pyo3(signature = (
        clustering = "color-cluster",
        hierarchical = "stacked",
        mode = "spline",
        filter_speckle = 4,
        color_precision = 6,
        layer_difference = 16,
        corner_threshold = 60,
        length_threshold = 4.0,
        max_iterations = 10,
        splice_threshold = 45,
        simplify = None,
        path_precision = 2,
        palette = None,
        max_colors = None,
        optimize = 1,
        binary_threshold = 128,
        adaptive = false,
        adaptive_window = 0,
        adaptive_t = 15.0,
        watershed_detail = 128,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        clustering: &str,
        hierarchical: &str,
        mode: &str,
        filter_speckle: usize,
        color_precision: i32,
        layer_difference: i32,
        corner_threshold: i32,
        length_threshold: f64,
        max_iterations: usize,
        splice_threshold: i32,
        simplify: Option<f64>,
        path_precision: u32,
        palette: Option<Vec<String>>,
        max_colors: Option<usize>,
        optimize: u8,
        binary_threshold: u8,
        adaptive: bool,
        adaptive_window: u32,
        adaptive_t: f64,
        watershed_detail: u32,
    ) -> PyResult<Self> {
        let palette = match palette {
            Some(list) => list.iter().map(|s| parse_hex(s)).collect::<PyResult<_>>()?,
            None => Vec::new(),
        };
        Ok(Self {
            inner: CoreConfig {
                clustering: parse(clustering)?,
                hierarchical: parse(hierarchical)?,
                mode: parse(mode)?,
                filter_speckle,
                color_precision,
                layer_difference,
                corner_threshold,
                length_threshold,
                max_iterations,
                splice_threshold,
                simplify,
                path_precision: Some(path_precision),
                palette,
                max_colors,
                optimize,
                binary_threshold,
                binary_adaptive: adaptive,
                binary_adaptive_window: adaptive_window,
                binary_adaptive_t: adaptive_t,
                watershed_detail,
            },
        })
    }

    /// Preset for black & white line art.
    #[staticmethod]
    fn bw() -> Self {
        Self {
            inner: CoreConfig::from_preset(Preset::Bw),
        }
    }

    /// Preset for posterized color art.
    #[staticmethod]
    fn poster() -> Self {
        Self {
            inner: CoreConfig::from_preset(Preset::Poster),
        }
    }

    /// Preset tuned for photographs.
    #[staticmethod]
    fn photo() -> Self {
        Self {
            inner: CoreConfig::from_preset(Preset::Photo),
        }
    }

    // --- properties ---

    #[getter]
    fn clustering(&self) -> &'static str {
        clustering_str(self.inner.clustering)
    }
    #[setter]
    fn set_clustering(&mut self, v: &str) -> PyResult<()> {
        self.inner.clustering = parse(v)?;
        Ok(())
    }

    #[getter]
    fn watershed_detail(&self) -> u32 {
        self.inner.watershed_detail
    }
    #[setter]
    fn set_watershed_detail(&mut self, v: u32) {
        self.inner.watershed_detail = v;
    }

    #[getter]
    fn hierarchical(&self) -> &'static str {
        hierarchical_str(self.inner.hierarchical)
    }
    #[setter]
    fn set_hierarchical(&mut self, v: &str) -> PyResult<()> {
        self.inner.hierarchical = parse(v)?;
        Ok(())
    }

    #[getter]
    fn mode(&self) -> &'static str {
        mode_str(self.inner.mode)
    }
    #[setter]
    fn set_mode(&mut self, v: &str) -> PyResult<()> {
        self.inner.mode = parse(v)?;
        Ok(())
    }

    #[getter]
    fn filter_speckle(&self) -> usize {
        self.inner.filter_speckle
    }
    #[setter]
    fn set_filter_speckle(&mut self, v: usize) {
        self.inner.filter_speckle = v;
    }

    #[getter]
    fn color_precision(&self) -> i32 {
        self.inner.color_precision
    }
    #[setter]
    fn set_color_precision(&mut self, v: i32) {
        self.inner.color_precision = v;
    }

    #[getter]
    fn layer_difference(&self) -> i32 {
        self.inner.layer_difference
    }
    #[setter]
    fn set_layer_difference(&mut self, v: i32) {
        self.inner.layer_difference = v;
    }

    #[getter]
    fn corner_threshold(&self) -> i32 {
        self.inner.corner_threshold
    }
    #[setter]
    fn set_corner_threshold(&mut self, v: i32) {
        self.inner.corner_threshold = v;
    }

    #[getter]
    fn length_threshold(&self) -> f64 {
        self.inner.length_threshold
    }
    #[setter]
    fn set_length_threshold(&mut self, v: f64) {
        self.inner.length_threshold = v;
    }

    #[getter]
    fn max_iterations(&self) -> usize {
        self.inner.max_iterations
    }
    #[setter]
    fn set_max_iterations(&mut self, v: usize) {
        self.inner.max_iterations = v;
    }

    #[getter]
    fn splice_threshold(&self) -> i32 {
        self.inner.splice_threshold
    }
    #[setter]
    fn set_splice_threshold(&mut self, v: i32) {
        self.inner.splice_threshold = v;
    }

    #[getter]
    fn simplify(&self) -> Option<f64> {
        self.inner.simplify
    }
    #[setter]
    fn set_simplify(&mut self, v: Option<f64>) {
        self.inner.simplify = v;
    }

    #[getter]
    fn path_precision(&self) -> Option<u32> {
        self.inner.path_precision
    }
    #[setter]
    fn set_path_precision(&mut self, v: Option<u32>) {
        self.inner.path_precision = v;
    }

    #[getter]
    fn palette(&self) -> Vec<String> {
        self.inner
            .palette
            .iter()
            .map(Color::to_hex_string)
            .collect()
    }
    #[setter]
    fn set_palette(&mut self, v: Vec<String>) -> PyResult<()> {
        self.inner.palette = v.iter().map(|s| parse_hex(s)).collect::<PyResult<_>>()?;
        Ok(())
    }

    #[getter]
    fn max_colors(&self) -> Option<usize> {
        self.inner.max_colors
    }
    #[setter]
    fn set_max_colors(&mut self, v: Option<usize>) {
        self.inner.max_colors = v;
    }

    #[getter]
    fn optimize(&self) -> u8 {
        self.inner.optimize
    }
    #[setter]
    fn set_optimize(&mut self, v: u8) {
        self.inner.optimize = v;
    }

    #[getter]
    fn binary_threshold(&self) -> u8 {
        self.inner.binary_threshold
    }
    #[setter]
    fn set_binary_threshold(&mut self, v: u8) {
        self.inner.binary_threshold = v;
    }

    #[getter]
    fn adaptive(&self) -> bool {
        self.inner.binary_adaptive
    }
    #[setter]
    fn set_adaptive(&mut self, v: bool) {
        self.inner.binary_adaptive = v;
    }

    #[getter]
    fn adaptive_window(&self) -> u32 {
        self.inner.binary_adaptive_window
    }
    #[setter]
    fn set_adaptive_window(&mut self, v: u32) {
        self.inner.binary_adaptive_window = v;
    }

    #[getter]
    fn adaptive_t(&self) -> f64 {
        self.inner.binary_adaptive_t
    }
    #[setter]
    fn set_adaptive_t(&mut self, v: f64) {
        self.inner.binary_adaptive_t = v;
    }

    // --- conversion ---

    /// Trace the image at `input_path` and write the SVG to `output_path`.
    fn convert_file(&self, input_path: PathBuf, output_path: PathBuf) -> PyResult<()> {
        let img = image::open(&input_path).map_err(|e| {
            PyIOError::new_err(format!("cannot open `{}`: {e}", input_path.display()))
        })?;
        let svg = self.to_svg(&dynimg_to_color(img))?;
        std::fs::write(&output_path, svg).map_err(|e| {
            PyIOError::new_err(format!("cannot write `{}`: {e}", output_path.display()))
        })
    }

    /// Trace encoded image `data` (png/jpg/...) and return the SVG string.
    /// `format` (e.g. "png") overrides content-based format detection.
    #[pyo3(signature = (data, format = None))]
    fn convert_bytes(&self, data: Vec<u8>, format: Option<&str>) -> PyResult<String> {
        self.to_svg(&decode_bytes(&data, format)?)
    }

    /// Trace a raw RGBA8 buffer (`width * height * 4` bytes) and return the SVG.
    fn convert_pixels(&self, rgba: Vec<u8>, width: usize, height: usize) -> PyResult<String> {
        if rgba.len() != width * height * 4 {
            return Err(PyValueError::new_err(format!(
                "rgba length {} != width*height*4 ({})",
                rgba.len(),
                width * height * 4
            )));
        }
        self.to_svg(&ColorImage {
            pixels: rgba,
            width,
            height,
        })
    }

    fn __repr__(&self) -> String {
        let c = &self.inner;
        format!(
            "Config(clustering='{}', hierarchical='{}', mode='{}', filter_speckle={}, \
             color_precision={}, layer_difference={}, corner_threshold={}, length_threshold={}, \
             max_iterations={}, splice_threshold={}, path_precision={:?}, palette={} colors, \
             max_colors={:?}, optimize={})",
            clustering_str(c.clustering),
            hierarchical_str(c.hierarchical),
            mode_str(c.mode),
            c.filter_speckle,
            c.color_precision,
            c.layer_difference,
            c.corner_threshold,
            c.length_threshold,
            c.max_iterations,
            c.splice_threshold,
            c.path_precision,
            c.palette.len(),
            c.max_colors,
            c.optimize,
        )
    }
}

// --- module-level convenience ------------------------------------------------

/// Convert a file to SVG on disk, using `config` (or defaults).
#[pyfunction]
#[pyo3(signature = (input_path, output_path, config = None))]
fn convert_file(
    input_path: PathBuf,
    output_path: PathBuf,
    config: Option<PyConfig>,
) -> PyResult<()> {
    config
        .unwrap_or_else(default_config)
        .convert_file(input_path, output_path)
}

/// Convert encoded image bytes to an SVG string, using `config` (or defaults).
#[pyfunction]
#[pyo3(signature = (data, config = None, format = None))]
fn convert_bytes(
    data: Vec<u8>,
    config: Option<PyConfig>,
    format: Option<&str>,
) -> PyResult<String> {
    config
        .unwrap_or_else(default_config)
        .convert_bytes(data, format)
}

/// Convert a raw RGBA8 buffer to an SVG string, using `config` (or defaults).
#[pyfunction]
#[pyo3(signature = (rgba, width, height, config = None))]
fn convert_pixels(
    rgba: Vec<u8>,
    width: usize,
    height: usize,
    config: Option<PyConfig>,
) -> PyResult<String> {
    config
        .unwrap_or_else(default_config)
        .convert_pixels(rgba, width, height)
}

fn default_config() -> PyConfig {
    PyConfig {
        inner: CoreConfig::default(),
    }
}

#[pymodule]
#[pyo3(name = "vtracer")]
fn vtracer_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyConfig>()?;
    m.add_function(wrap_pyfunction!(convert_file, m)?)?;
    m.add_function(wrap_pyfunction!(convert_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(convert_pixels, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}

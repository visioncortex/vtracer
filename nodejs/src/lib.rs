//! WebAssembly core for the vtracer Node package.
//!
//! Exposes vectorization over encoded image bytes or a raw RGBA buffer. Image
//! decoding happens here (in wasm), so the JS layer only needs `fs` — no
//! native dependency. Options are a plain JS object matching [`Options`].

use std::io::Cursor;

use serde::Deserialize;
use vtracer::{Color, ColorImage, Config};
use wasm_bindgen::prelude::*;

/// Conversion options; a subset may be provided from JS (camelCase). Anything
/// omitted uses the framework default.
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Options {
    /// Region forming: "color-cluster" | "bw" | "watershed".
    clustering: Option<String>,
    hierarchical: Option<String>,
    mode: Option<String>,
    filter_speckle: Option<usize>,
    color_precision: Option<i32>,
    layer_difference: Option<i32>,
    corner_threshold: Option<i32>,
    length_threshold: Option<f64>,
    max_iterations: Option<usize>,
    splice_threshold: Option<i32>,
    /// Curve simplification tolerance in px (omit = off).
    simplify: Option<f64>,
    path_precision: Option<u32>,
    palette: Option<Vec<String>>,
    max_colors: Option<usize>,
    optimize: Option<u8>,
    /// Binary-mode fixed threshold (0..=255).
    binary_threshold: Option<u8>,
    /// Binary mode: use Bradley–Roth adaptive thresholding.
    adaptive: Option<bool>,
    /// Adaptive window side length in px (0 = auto).
    adaptive_window: Option<u32>,
    /// Adaptive sensitivity: percent below the local mean (default 15).
    adaptive_t: Option<f64>,
    /// Watershed clustering: hierarchy cut level (default 128; higher = more
    /// regions, uncapped).
    watershed_detail: Option<u32>,
    /// One of "bw" | "poster" | "photo"; applied before the other fields.
    preset: Option<String>,
}

fn err(msg: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&msg.to_string())
}

fn parse_hex(token: &str) -> Result<Color, JsValue> {
    let hex = token.strip_prefix('#').unwrap_or(token);
    if hex.len() != 6 {
        return Err(err(format!("`{token}` is not a #rrggbb color")));
    }
    let b = |r: std::ops::Range<usize>| {
        u8::from_str_radix(&hex[r], 16).map_err(|_| err(format!("`{token}` is not a #rrggbb color")))
    };
    Ok(Color::new(b(0..2)?, b(2..4)?, b(4..6)?))
}

fn config_from(options: JsValue) -> Result<Config, JsValue> {
    let opts: Options = if options.is_undefined() || options.is_null() {
        Options::default()
    } else {
        serde_wasm_bindgen::from_value(options).map_err(err)?
    };

    let mut config = match opts.preset.as_deref() {
        Some("bw") => Config::from_preset(vtracer::Preset::Bw),
        Some("poster") => Config::from_preset(vtracer::Preset::Poster),
        Some("photo") => Config::from_preset(vtracer::Preset::Photo),
        Some(other) => return Err(err(format!("unknown preset `{other}`"))),
        None => Config::default(),
    };

    if let Some(v) = opts.clustering {
        config.clustering = v.parse().map_err(err)?;
    }
    if let Some(v) = opts.hierarchical {
        config.hierarchical = v.parse().map_err(err)?;
    }
    if let Some(v) = opts.mode {
        config.mode = v.parse().map_err(err)?;
    }
    if let Some(v) = opts.filter_speckle {
        config.filter_speckle = v;
    }
    if let Some(v) = opts.color_precision {
        config.color_precision = v;
    }
    if let Some(v) = opts.layer_difference {
        config.layer_difference = v;
    }
    if let Some(v) = opts.corner_threshold {
        config.corner_threshold = v;
    }
    if let Some(v) = opts.length_threshold {
        config.length_threshold = v;
    }
    if let Some(v) = opts.max_iterations {
        config.max_iterations = v;
    }
    if let Some(v) = opts.splice_threshold {
        config.splice_threshold = v;
    }
    if let Some(v) = opts.simplify {
        config.simplify = Some(v);
    }
    if let Some(v) = opts.path_precision {
        config.path_precision = Some(v);
    }
    if let Some(list) = opts.palette {
        config.palette = list.iter().map(|s| parse_hex(s)).collect::<Result<_, _>>()?;
    }
    if let Some(v) = opts.max_colors {
        config.max_colors = Some(v);
    }
    if let Some(v) = opts.optimize {
        config.optimize = v;
    }
    if let Some(v) = opts.binary_threshold {
        config.binary_threshold = v;
    }
    // Any adaptive tuning field (or `adaptive: true`) switches on Bradley–Roth.
    if opts.adaptive == Some(true) || opts.adaptive_window.is_some() || opts.adaptive_t.is_some() {
        config.binary_adaptive = true;
    }
    if let Some(v) = opts.adaptive_window {
        config.binary_adaptive_window = v;
    }
    if let Some(v) = opts.adaptive_t {
        config.binary_adaptive_t = v;
    }
    if let Some(v) = opts.watershed_detail {
        config.watershed_detail = v;
    }
    Ok(config)
}

fn to_svg(config: Config, img: ColorImage) -> Result<String, JsValue> {
    config.build().map_err(err)?.to_svg(&img).map_err(err)
}

/// Vectorize encoded image bytes (PNG/JPEG/GIF/BMP). Returns the SVG string.
#[wasm_bindgen]
pub fn vectorize_bytes(data: &[u8], options: JsValue) -> Result<String, JsValue> {
    let config = config_from(options)?;
    let img = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(err)?
        .decode()
        .map_err(|e| err(format!("failed to decode image: {e}")))?
        .to_rgba8();
    let (width, height) = (img.width() as usize, img.height() as usize);
    to_svg(
        config,
        ColorImage {
            pixels: img.into_raw(),
            width,
            height,
        },
    )
}

/// Vectorize a raw RGBA8 buffer (`width * height * 4` bytes). Returns the SVG.
#[wasm_bindgen]
pub fn vectorize_rgba(
    data: Vec<u8>,
    width: usize,
    height: usize,
    options: JsValue,
) -> Result<String, JsValue> {
    if data.len() != width * height * 4 {
        return Err(err(format!(
            "rgba length {} != width*height*4 ({})",
            data.len(),
            width * height * 4
        )));
    }
    let config = config_from(options)?;
    to_svg(
        config,
        ColorImage {
            pixels: data,
            width,
            height,
        },
    )
}

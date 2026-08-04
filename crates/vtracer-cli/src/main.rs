//! Thin command-line front-end over the `vtracer` framework.
//!
//! Handles the two things the framework deliberately leaves out: image file
//! I/O and argument parsing. Everything else is delegated to
//! [`vtracer::Config`] / [`vtracer::Pipeline`].

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use visioncortex::{Color, ColorImage};
use vtracer::{Clustering, Config, FitMode, Hierarchical, Preset};

/// Convert an image into vector graphics.
#[derive(Parser, Debug)]
#[command(name = "vtracer", version, about, rename_all = "kebab-case")]
struct Args {
    /// Input raster image (positional; or use --input).
    #[arg(value_name = "INPUT")]
    input_pos: Option<PathBuf>,

    /// Output SVG (positional; or use --output).
    #[arg(value_name = "OUTPUT")]
    output_pos: Option<PathBuf>,

    /// Path to the input raster image.
    #[arg(short = 'i', long = "input", value_name = "INPUT")]
    input: Option<PathBuf>,

    /// Path to the output SVG.
    #[arg(short = 'o', long = "output", value_name = "OUTPUT")]
    output: Option<PathBuf>,

    /// Start from a preset: bw, poster, photo.
    #[arg(long)]
    preset: Option<Preset>,

    /// Region forming: `color-cluster` (default), `bw`, or `watershed`.
    #[arg(long)]
    clustering: Option<Clustering>,

    /// Hierarchical clustering: `stacked` (default) or `cutout` (mosaic).
    #[arg(long)]
    hierarchical: Option<Hierarchical>,

    /// Curve-fitting mode: pixel, polygon, spline.
    #[arg(short, long)]
    mode: Option<FitMode>,

    /// Discard patches smaller than X px in size (0..=128).
    #[arg(short = 'f', long, value_parser = clap::value_parser!(i64).range(0..=128))]
    filter_speckle: Option<i64>,

    /// Significant bits per RGB channel (1..=8).
    #[arg(short = 'p', long, value_parser = clap::value_parser!(i64).range(1..=8))]
    color_precision: Option<i64>,

    /// Color difference between gradient layers (0..=255).
    #[arg(short = 'g', long, value_parser = clap::value_parser!(i64).range(0..=255))]
    gradient_step: Option<i64>,

    /// Minimum momentary angle (degrees) to be a corner (0..=180).
    ///
    /// Hidden from help: a fine-tuning knob few conversions need — the
    /// default (60) serves; `--simplify` is the knob worth reaching for.
    #[arg(long, hide = true, value_parser = clap::value_parser!(i64).range(0..=180))]
    corner_threshold: Option<i64>,

    /// Subdivide until all segments are shorter than this length (3.5..=10).
    ///
    /// Hidden from help: with `--simplify` reducing anchors by an explicit
    /// error tolerance, this legacy knob's effect on output is negligible.
    #[arg(long, hide = true, value_parser = parse_segment_length)]
    segment_length: Option<f64>,

    /// Minimum angle displacement (degrees) to splice a spline (0..=180).
    ///
    /// Hidden from help: a fine-tuning knob few conversions need — the
    /// default (45) serves; `--simplify` is the knob worth reaching for.
    #[arg(long, hide = true, value_parser = clap::value_parser!(i64).range(0..=180))]
    splice_threshold: Option<i64>,

    /// Simplify curves: fewest cubics within this tolerance in px (try 1-2.5).
    #[arg(long, value_name = "TOLERANCE", value_parser = parse_simplify_tolerance)]
    simplify: Option<f64>,

    /// Decimal places to use in path coordinates.
    #[arg(long)]
    path_precision: Option<u32>,

    /// Fixed palette: comma-separated hex colors, e.g. '#112233,#445566'.
    #[arg(long)]
    palette: Option<String>,

    /// Fixed palette from a file (one hex color per line or comma-separated).
    #[arg(long)]
    palette_file: Option<PathBuf>,

    /// Auto-quantize to at most N colors.
    #[arg(long)]
    max_colors: Option<usize>,

    /// Optimization level: 0 = off, 1 = quantize+cleanup, 2 = + shorthands/grouping.
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=2))]
    optimize: Option<u8>,

    /// Binary mode: fixed threshold (0..=255); foreground when intensity is below it.
    #[arg(long, value_parser = clap::value_parser!(u8))]
    threshold: Option<u8>,

    /// Binary mode: use Bradley–Roth adaptive thresholding (handles uneven lighting).
    #[arg(long)]
    adaptive: bool,

    /// Adaptive window side length in px (0 = auto). Implies --adaptive.
    #[arg(long)]
    adaptive_window: Option<u32>,

    /// Adaptive sensitivity: percent below the local mean (default 15). Implies --adaptive.
    #[arg(long)]
    adaptive_t: Option<f64>,

    /// Watershed clustering: hierarchy cut level (0..=255, higher = more regions).
    #[arg(long, value_parser = clap::value_parser!(u32))]
    watershed_detail: Option<u32>,
}

fn parse_simplify_tolerance(s: &str) -> Result<f64, String> {
    let v: f64 = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
    if !v.is_finite() || v <= 0.0 {
        return Err(format!("simplify tolerance {v} must be positive"));
    }
    Ok(v)
}

fn parse_segment_length(s: &str) -> Result<f64, String> {
    let v: f64 = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
    if !(3.5..=10.0).contains(&v) {
        return Err(format!("segment length {v} is out of range [3.5, 10]"));
    }
    Ok(v)
}

/// Parse a comma/whitespace/newline separated list of `#rrggbb` colors.
fn parse_palette(text: &str) -> Result<Vec<Color>, String> {
    let mut colors = Vec::new();
    for token in text.split(|c: char| c == ',' || c.is_whitespace()) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        colors.push(parse_hex_color(token)?);
    }
    Ok(colors)
}

fn parse_hex_color(token: &str) -> Result<Color, String> {
    let hex = token.strip_prefix('#').unwrap_or(token);
    if hex.len() != 6 {
        return Err(format!("`{token}` is not a #rrggbb color"));
    }
    let parse = |range: std::ops::Range<usize>| {
        u8::from_str_radix(&hex[range], 16).map_err(|_| format!("`{token}` is not a #rrggbb color"))
    };
    Ok(Color::new(parse(0..2)?, parse(2..4)?, parse(4..6)?))
}

fn build_config(args: &Args) -> Result<Config, String> {
    let mut config = match args.preset {
        Some(preset) => Config::from_preset(preset),
        None => Config::default(),
    };

    if let Some(v) = args.clustering {
        config.clustering = v;
    }
    if let Some(v) = args.hierarchical {
        config.hierarchical = v;
    }
    if let Some(v) = args.mode {
        config.mode = v;
    }
    if let Some(v) = args.filter_speckle {
        config.filter_speckle = v as usize;
    }
    if let Some(v) = args.color_precision {
        config.color_precision = v as i32;
    }
    if let Some(v) = args.gradient_step {
        config.layer_difference = v as i32;
    }
    if let Some(v) = args.corner_threshold {
        config.corner_threshold = v as i32;
    }
    if let Some(v) = args.segment_length {
        config.length_threshold = v;
    }
    if let Some(v) = args.splice_threshold {
        config.splice_threshold = v as i32;
    }
    if args.simplify.is_some() {
        config.simplify = args.simplify;
    }
    if args.path_precision.is_some() {
        config.path_precision = args.path_precision;
    }
    if let Some(v) = args.optimize {
        config.optimize = v;
    }
    if let Some(v) = args.max_colors {
        config.max_colors = Some(v);
    }

    // Binary thresholding: --adaptive (or either adaptive tuning flag) selects
    // Bradley–Roth; otherwise --threshold tunes the fixed cutoff.
    if let Some(v) = args.threshold {
        config.binary_threshold = v;
    }
    if args.adaptive || args.adaptive_window.is_some() || args.adaptive_t.is_some() {
        config.binary_adaptive = true;
    }
    if let Some(v) = args.adaptive_window {
        config.binary_adaptive_window = v;
    }
    if let Some(v) = args.adaptive_t {
        config.binary_adaptive_t = v;
    }
    if let Some(v) = args.watershed_detail {
        config.watershed_detail = v;
    }

    // Palette: inline flag wins over file; both parse to a color list.
    if let Some(text) = &args.palette {
        config.palette = parse_palette(text)?;
    } else if let Some(path) = &args.palette_file {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("cannot read palette file: {e}"))?;
        config.palette = parse_palette(&text)?;
    }

    Ok(config)
}

fn read_image(path: &std::path::Path) -> Result<ColorImage, String> {
    let img = image::open(path)
        .map_err(|_| "no image file found at specified input path".to_string())?
        .to_rgba8();
    let (width, height) = (img.width() as usize, img.height() as usize);
    Ok(ColorImage {
        pixels: img.into_raw(),
        width,
        height,
    })
}

fn run() -> Result<(), String> {
    let args = Args::parse();

    // Accept input/output as positionals (`vtracer in.png out.svg`) or as
    // named flags; an explicit flag takes precedence over the positional.
    let input = args
        .input
        .as_ref()
        .or(args.input_pos.as_ref())
        .ok_or("no input path given (positional or --input)")?;
    let output = args
        .output
        .as_ref()
        .or(args.output_pos.as_ref())
        .ok_or("no output path given (positional or --output)")?;

    let config = build_config(&args)?;
    let pipeline = config.build().map_err(|e| e.to_string())?;
    let img = read_image(input)?;
    let svg = pipeline.to_svg(&img).map_err(|e| e.to_string())?;
    std::fs::write(output, svg).map_err(|e| format!("cannot write output file: {e}"))?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => {
            println!("Conversion successful.");
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("Conversion failed: {msg}");
            ExitCode::FAILURE
        }
    }
}

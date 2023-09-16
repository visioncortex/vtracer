use crate::*;
use pyo3::prelude::*;
use std::path::PathBuf;
use visioncortex::PathSimplifyMode;

/// Python binding
#[pyfunction]
fn convert_image_to_svg_py(
    image_path: &str,
    out_path: &str,
    colormode: Option<&str>,       // "color" or "binary"
    hierarchical: Option<&str>,    // "stacked" or "cutout"
    mode: Option<&str>,            // "polygon", "spline", "none"
    filter_speckle: Option<usize>, // default: 4
    color_precision: Option<i32>,  // default: 6
    layer_difference: Option<i32>, // default: 16
    corner_threshold: Option<i32>, // default: 60
    length_threshold: Option<f64>, // in [3.5, 10] default: 4.0
    max_iterations: Option<usize>, // default: 10
    splice_threshold: Option<i32>, // default: 45
    path_precision: Option<u32>,   // default: 8
) -> PyResult<()> {
    let input_path = PathBuf::from(image_path);
    let output_path = PathBuf::from(out_path);

    // TODO: enforce color mode with an enum so that we only
    // accept the strings 'color' or 'binary'
    let color_mode = match colormode.unwrap_or("color") {
        "color" => ColorMode::Color,
        "binary" => ColorMode::Binary,
        _ => ColorMode::Color,
    };

    let hierarchical = match hierarchical.unwrap_or("stacked") {
        "stacked" => Hierarchical::Stacked,
        "cutout" => Hierarchical::Cutout,
        _ => Hierarchical::Stacked,
    };

    let mode = match mode.unwrap_or("spline") {
        "spline" => PathSimplifyMode::Spline,
        "polygon" => PathSimplifyMode::Polygon,
        "none" => PathSimplifyMode::None,
        _ => PathSimplifyMode::Spline,
    };

    let filter_speckle = filter_speckle.unwrap_or(4);
    let color_precision = color_precision.unwrap_or(6);
    let layer_difference = layer_difference.unwrap_or(16);
    let corner_threshold = corner_threshold.unwrap_or(60);
    let length_threshold = length_threshold.unwrap_or(4.0);
    let splice_threshold = splice_threshold.unwrap_or(45);
    let max_iterations = max_iterations.unwrap_or(10);

    let config = Config {
        input_path,
        output_path,
        color_mode,
        hierarchical,
        filter_speckle,
        color_precision,
        layer_difference,
        mode,
        corner_threshold,
        length_threshold,
        max_iterations,
        splice_threshold,
        path_precision,
        ..Default::default()
    };

    convert_image_to_svg(config).unwrap();
    Ok(())
}

/// A Python module implemented in Rust.
#[pymodule]
fn vtracer(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(convert_image_to_svg_py, m)?)?;
    Ok(())
}

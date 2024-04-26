use crate::*;
use image::{io::Reader, ImageFormat};
use pyo3::{exceptions::PyException, prelude::*};
use std::io::{BufReader, Cursor};
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

    let config = construct_config(
        colormode,
        hierarchical,
        mode,
        filter_speckle,
        color_precision,
        layer_difference,
        corner_threshold,
        length_threshold,
        max_iterations,
        splice_threshold,
        path_precision,
    );

    convert_image_to_svg(&input_path, &output_path, config).unwrap();
    Ok(())
}

#[pyfunction]
fn convert_raw_image_to_svg(
    img_bytes: Vec<u8>,
    img_format: Option<&str>, // Format of the image (e.g. 'jpg', 'png'... A full list of supported formats can be found [here](https://docs.rs/image/latest/image/enum.ImageFormat.html)). If not provided, the image format will be guessed based on its contents.
    colormode: Option<&str>,  // "color" or "binary"
    hierarchical: Option<&str>, // "stacked" or "cutout"
    mode: Option<&str>,       // "polygon", "spline", "none"
    filter_speckle: Option<usize>, // default: 4
    color_precision: Option<i32>, // default: 6
    layer_difference: Option<i32>, // default: 16
    corner_threshold: Option<i32>, // default: 60
    length_threshold: Option<f64>, // in [3.5, 10] default: 4.0
    max_iterations: Option<usize>, // default: 10
    splice_threshold: Option<i32>, // default: 45
    path_precision: Option<u32>, // default: 8
) -> PyResult<String> {
    let config = construct_config(
        colormode,
        hierarchical,
        mode,
        filter_speckle,
        color_precision,
        layer_difference,
        corner_threshold,
        length_threshold,
        max_iterations,
        splice_threshold,
        path_precision,
    );
    let mut img_reader = Reader::new(BufReader::new(Cursor::new(img_bytes)));
    let img_format = img_format.and_then(|ext_name| ImageFormat::from_extension(ext_name));
    let img = match img_format {
        Some(img_format) => {
            img_reader.set_format(img_format);
            img_reader.decode()
        }
        None => img_reader
            .with_guessed_format()
            .map_err(|_| PyException::new_err("Unrecognized image format. "))?
            .decode(),
    };
    let img = match img {
        Ok(img) => img.to_rgba8(),
        Err(_) => return Err(PyException::new_err("Failed to decode img_bytes. ")),
    };
    let (width, height) = (img.width() as usize, img.height() as usize);
    let img = ColorImage {
        pixels: img.as_raw().to_vec(),
        width,
        height,
    };
    let svg =
        convert(img, config).map_err(|_| PyException::new_err("Failed to convert the image. "))?;
    Ok(format!("{}", svg))
}

#[pyfunction]
fn convert_pixels_to_svg(
    rgba_pixels: Vec<(u8, u8, u8, u8)>,
    size: (usize, usize),
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
) -> PyResult<String> {
    let expected_pixel_count = size.0 * size.1;
    if rgba_pixels.len() != expected_pixel_count {
        return Err(PyException::new_err(format!(
            "Length of rgba_pixels does not match given image size. Expected {} ({} * {}), got {}. ",
            expected_pixel_count,
            size.0,
            size.1,
            rgba_pixels.len()
        )));
    }
    let config = construct_config(
        colormode,
        hierarchical,
        mode,
        filter_speckle,
        color_precision,
        layer_difference,
        corner_threshold,
        length_threshold,
        max_iterations,
        splice_threshold,
        path_precision,
    );
    let mut flat_pixels: Vec<u8> = vec![];
    for (r, g, b, a) in rgba_pixels {
        flat_pixels.push(r);
        flat_pixels.push(g);
        flat_pixels.push(b);
        flat_pixels.push(a);
    }
    let mut img = ColorImage::new();
    img.pixels = flat_pixels;
    (img.width, img.height) = size;

    let svg =
        convert(img, config).map_err(|_| PyException::new_err("Failed to convert the image. "))?;
    Ok(format!("{}", svg))
}

fn construct_config(
    colormode: Option<&str>,
    hierarchical: Option<&str>,
    mode: Option<&str>,
    filter_speckle: Option<usize>,
    color_precision: Option<i32>,
    layer_difference: Option<i32>,
    corner_threshold: Option<i32>,
    length_threshold: Option<f64>,
    max_iterations: Option<usize>,
    splice_threshold: Option<i32>,
    path_precision: Option<u32>,
) -> Config {
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

    Config {
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
    }
}

/// A Python module implemented in Rust.
#[pymodule]
fn vtracer(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(convert_image_to_svg_py, m)?)?;
    m.add_function(wrap_pyfunction!(convert_raw_image_to_svg, m)?)?;
    m.add_function(wrap_pyfunction!(convert_pixels_to_svg, m)?)?;
    Ok(())
}

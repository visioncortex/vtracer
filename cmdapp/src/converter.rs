use std::path::PathBuf;
use std::{fs::File, io::Write};
use pyo3::prelude::*;

use fastrand::Rng;
use visioncortex::{Color, ColorImage, ColorName, PathSimplifyMode};
use visioncortex::color_clusters::{Runner, RunnerConfig, KeyingAction, HIERARCHICAL_MAX};
use super::config::{Config, ColorMode, Hierarchical, ConverterConfig};
use super::svg::SvgFile;

const NUM_UNUSED_COLOR_ITERATIONS: usize = 6;
/// The fraction of pixels in the top/bottom rows of the image that need to be transparent before
/// the entire image will be keyed.
const KEYING_THRESHOLD: f32 = 0.2;

/// Convert an image file into svg file
pub fn convert_image_to_svg(config: Config) -> Result<(), String> {
    let config = config.into_converter_config();
    match config.color_mode {
        ColorMode::Color => color_image_to_svg(config),
        ColorMode::Binary => binary_image_to_svg(config),
    }
}

fn color_exists_in_image(img: &ColorImage, color: Color) -> bool {
    for y in 0..img.height {
        for x in 0..img.width {
            let pixel_color = img.get_pixel(x, y);
            if pixel_color.r == color.r && pixel_color.g == color.g && pixel_color.b == color.b {
                return true
            }
        }
    }
    false
}

fn find_unused_color_in_image(img: &ColorImage) -> Result<Color, String> {
    let special_colors = IntoIterator::into_iter([
        Color::new(255, 0,   0),
        Color::new(0,   255, 0),
        Color::new(0,   0,   255),
        Color::new(255, 255, 0),
        Color::new(0,   255, 255),
        Color::new(255, 0,   255),
    ]);
    let rng = Rng::new();
    let random_colors = (0..NUM_UNUSED_COLOR_ITERATIONS).map(|_| {
        Color::new(
            rng.u8(..),
            rng.u8(..),
            rng.u8(..),
        )
    });
    for color in special_colors.chain(random_colors) {
        if !color_exists_in_image(img, color) {
            return Ok(color);
        }
    }
    Err(String::from("unable to find unused color in image to use as key"))
}

fn should_key_image(img: &ColorImage) -> bool {
    if img.width == 0 || img.height == 0 {
        return false;
    }

    // Check for transparency at several scanlines
    let threshold = ((img.width * 2) as f32 * KEYING_THRESHOLD) as usize;
    let mut num_transparent_boundary_pixels = 0;
    let y_positions = [0, img.height / 4, img.height / 2, 3 * img.height / 4, img.height - 1];
    for y in y_positions {
        for x in 0..img.width {
            if img.get_pixel(x, y).a == 0 {
                num_transparent_boundary_pixels += 1;
            }
            if num_transparent_boundary_pixels >= threshold {
                return true;
            }
        }
    }

    false
}

fn color_image_to_svg(config: ConverterConfig) -> Result<(), String> {
    let (mut img, width, height);
    match read_image(config.input_path) {
        Ok(values) => {
            img = values.0;
            width = values.1;
            height = values.2;
        },
        Err(msg) => return Err(msg),
    }

    let key_color = if should_key_image(&img) {
        let key_color = find_unused_color_in_image(&img)?;
        for y in 0..height {
            for x in 0..width {
                if img.get_pixel(x, y).a == 0 {
                    img.set_pixel(x, y, &key_color);
                }
            }
        }
        key_color
    } else {
        // The default color is all zeroes, which is treated by visioncortex as a special value meaning no keying will be applied.
        Color::default()
    };

    let runner = Runner::new(RunnerConfig {
        diagonal: config.layer_difference == 0,
        hierarchical: HIERARCHICAL_MAX,
        batch_size: 25600,
        good_min_area: config.filter_speckle_area,
        good_max_area: (width * height),
        is_same_color_a: config.color_precision_loss,
        is_same_color_b: 1,
        deepen_diff: config.layer_difference,
        hollow_neighbours: 1,
        key_color,
        keying_action: if matches!(config.hierarchical, Hierarchical::Cutout) {
            KeyingAction::Keep
        } else {
            KeyingAction::Discard
        },
    }, img);

    let mut clusters = runner.run();

    match config.hierarchical {
        Hierarchical::Stacked => {}
        Hierarchical::Cutout => {
            let view = clusters.view();
            let image = view.to_color_image();
            let runner = Runner::new(RunnerConfig {
                diagonal: false,
                hierarchical: 64,
                batch_size: 25600,
                good_min_area: 0,
                good_max_area: (image.width * image.height) as usize,
                is_same_color_a: 0,
                is_same_color_b: 1,
                deepen_diff: 0,
                hollow_neighbours: 0,
                key_color,
                keying_action: KeyingAction::Discard,
            }, image);
            clusters = runner.run();
        },
    }

    let view = clusters.view();

    let mut svg = SvgFile::new(width, height, config.path_precision);
    for &cluster_index in view.clusters_output.iter().rev() {
        let cluster = view.get_cluster(cluster_index);
        let paths = cluster.to_compound_path(
            &view,
            false,
            config.mode,
            config.corner_threshold,
            config.length_threshold,
            config.max_iterations,
            config.splice_threshold
        );
        svg.add_path(paths, cluster.residue_color());
    }

    write_svg(svg, config.output_path)
}

fn binary_image_to_svg(config: ConverterConfig) -> Result<(), String> {

    let (img, width, height);
    match read_image(config.input_path) {
        Ok(values) => {
            img = values.0;
            width = values.1;
            height = values.2;
        },
        Err(msg) => return Err(msg),
    }
    let img = img.to_binary_image(|x| x.r < 128);

    let clusters = img.to_clusters(false);

    let mut svg = SvgFile::new(width, height, config.path_precision);
    for i in 0..clusters.len() {
        let cluster = clusters.get_cluster(i);
        if cluster.size() >= config.filter_speckle_area {
            let paths = cluster.to_compound_path(
                config.mode,
                config.corner_threshold,
                config.length_threshold,
                config.max_iterations,
                config.splice_threshold,
            );
            svg.add_path(paths, Color::color(&ColorName::Black));
        }
    }

    write_svg(svg, config.output_path)
}

fn read_image(input_path: PathBuf) -> Result<(ColorImage, usize, usize), String> {
    let img = image::open(input_path);
    let img = match img {
        Ok(file) => file.to_rgba8(),
        Err(_) => return Err(String::from("No image file found at specified input path")),
    };

    let (width, height) = (img.width() as usize, img.height() as usize);
    let img = ColorImage {pixels: img.as_raw().to_vec(), width, height};

    Ok((img, width, height))
}

fn write_svg(svg: SvgFile, output_path: PathBuf) -> Result<(), String> {
    let out_file = File::create(output_path);
    let mut out_file = match out_file {
        Ok(file) => file,
        Err(_) => return Err(String::from("Cannot create output file.")),
    };

    write!(&mut out_file, "{}", svg).expect("failed to write file.");

    Ok(())
}

// ==================
// = PYTHON BINDING =
// ==================
#[pyfunction]
fn convert_image_to_svg_py( image_path: &str, 
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
                            path_precision: Option<u32>    // default: 8
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

    let filter_speckle =    filter_speckle.unwrap_or(4);
    let color_precision =   color_precision.unwrap_or(6);
    let layer_difference =  layer_difference.unwrap_or(16);
    let corner_threshold =  corner_threshold.unwrap_or(60);
    let length_threshold =  length_threshold.unwrap_or(4.0);
    let splice_threshold =  splice_threshold.unwrap_or(45);
    let max_iterations =    max_iterations.unwrap_or(10);

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
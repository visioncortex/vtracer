mod config;
mod converter;
mod svg;

use clap::{App, Arg};
use config::{ColorMode, Config, Hierarchical, Preset};
use std::path::PathBuf;
use std::str::FromStr;
use visioncortex::PathSimplifyMode;

fn path_simplify_mode_from_str(s: &str) -> PathSimplifyMode {
    match s {
        "polygon" => PathSimplifyMode::Polygon,
        "spline" => PathSimplifyMode::Spline,
        "none" => PathSimplifyMode::None,
        _ => panic!("unknown PathSimplifyMode {}", s),
    }
}

pub fn config_from_args() -> (PathBuf, PathBuf, Config) {
    let app = App::new("visioncortex VTracer ".to_owned() + env!("CARGO_PKG_VERSION"))
        .about("A cmd app to convert images into vector graphics.");

    let app = app.arg(
        Arg::with_name("input")
            .long("input")
            .short("i")
            .takes_value(true)
            .help("Path to input raster image")
            .required(true),
    );

    let app = app.arg(
        Arg::with_name("output")
            .long("output")
            .short("o")
            .takes_value(true)
            .help("Path to output vector graphics")
            .required(true),
    );

    let app = app.arg(
        Arg::with_name("color_mode")
            .long("colormode")
            .takes_value(true)
            .help("True color image `color` (default) or Binary image `bw`"),
    );

    let app = app.arg(
        Arg::with_name("hierarchical")
            .long("hierarchical")
            .takes_value(true)
            .help(
                "Hierarchical clustering `stacked` (default) or non-stacked `cutout`. \
            Only applies to color mode. ",
            ),
    );

    let app = app.arg(
        Arg::with_name("preset")
            .long("preset")
            .takes_value(true)
            .help("Use one of the preset configs `bw`, `poster`, `photo`"),
    );

    let app = app.arg(
        Arg::with_name("filter_speckle")
            .long("filter_speckle")
            .short("f")
            .takes_value(true)
            .help("Discard patches smaller than X px in size"),
    );

    let app = app.arg(
        Arg::with_name("color_precision")
            .long("color_precision")
            .short("p")
            .takes_value(true)
            .help("Number of significant bits to use in an RGB channel"),
    );

    let app = app.arg(
        Arg::with_name("gradient_step")
            .long("gradient_step")
            .short("g")
            .takes_value(true)
            .help("Color difference between gradient layers"),
    );

    let app = app.arg(
        Arg::with_name("corner_threshold")
            .long("corner_threshold")
            .short("c")
            .takes_value(true)
            .help("Minimum momentary angle (degree) to be considered a corner"),
    );

    let app = app.arg(Arg::with_name("segment_length")
        .long("segment_length")
        .short("l")
        .takes_value(true)
        .help("Perform iterative subdivide smooth until all segments are shorter than this length"));

    let app = app.arg(
        Arg::with_name("splice_threshold")
            .long("splice_threshold")
            .short("s")
            .takes_value(true)
            .help("Minimum angle displacement (degree) to splice a spline"),
    );

    let app = app.arg(
        Arg::with_name("mode")
            .long("mode")
            .short("m")
            .takes_value(true)
            .help("Curver fitting mode `pixel`, `polygon`, `spline`"),
    );

    let app = app.arg(
        Arg::with_name("path_precision")
            .long("path_precision")
            .takes_value(true)
            .help("Number of decimal places to use in path string"),
    );

    // Extract matches
    let matches = app.get_matches();

    let mut config = Config::default();
    let input_path = matches
        .value_of("input")
        .expect("Input path is required, please specify it by --input or -i.");
    let output_path = matches
        .value_of("output")
        .expect("Output path is required, please specify it by --output or -o.");

    let input_path = PathBuf::from(input_path);
    let output_path = PathBuf::from(output_path);

    if let Some(value) = matches.value_of("preset") {
        config = Config::from_preset(Preset::from_str(value).unwrap());
    }

    if let Some(value) = matches.value_of("color_mode") {
        config.color_mode = ColorMode::from_str(if value.trim() == "bw" || value.trim() == "BW" {
            "binary"
        } else {
            "color"
        })
        .unwrap()
    }

    if let Some(value) = matches.value_of("hierarchical") {
        config.hierarchical = Hierarchical::from_str(value).unwrap()
    }

    if let Some(value) = matches.value_of("mode") {
        let value = value.trim();
        config.mode = path_simplify_mode_from_str(if value == "pixel" {
            "none"
        } else if value == "polygon" {
            "polygon"
        } else if value == "spline" {
            "spline"
        } else {
            panic!("Parser Error: Curve fitting mode is invalid: {}", value);
        });
    }

    if let Some(value) = matches.value_of("filter_speckle") {
        if value.trim().parse::<usize>().is_ok() {
            // is numeric
            let value = value.trim().parse::<usize>().unwrap();
            if value > 16 {
                panic!("Out of Range Error: Filter speckle is invalid at {}. It must be within [0,16].", value);
            }
            config.filter_speckle = value;
        } else {
            panic!(
                "Parser Error: Filter speckle is not a positive integer: {}.",
                value
            );
        }
    }

    if let Some(value) = matches.value_of("color_precision") {
        if value.trim().parse::<i32>().is_ok() {
            // is numeric
            let value = value.trim().parse::<i32>().unwrap();
            if value < 1 || value > 8 {
                panic!("Out of Range Error: Color precision is invalid at {}. It must be within [1,8].", value);
            }
            config.color_precision = value;
        } else {
            panic!(
                "Parser Error: Color precision is not an integer: {}.",
                value
            );
        }
    }

    if let Some(value) = matches.value_of("gradient_step") {
        if value.trim().parse::<i32>().is_ok() {
            // is numeric
            let value = value.trim().parse::<i32>().unwrap();
            if value < 0 || value > 255 {
                panic!("Out of Range Error: Gradient step is invalid at {}. It must be within [0,255].", value);
            }
            config.layer_difference = value;
        } else {
            panic!("Parser Error: Gradient step is not an integer: {}.", value);
        }
    }

    if let Some(value) = matches.value_of("corner_threshold") {
        if value.trim().parse::<i32>().is_ok() {
            // is numeric
            let value = value.trim().parse::<i32>().unwrap();
            if value < 0 || value > 180 {
                panic!("Out of Range Error: Corner threshold is invalid at {}. It must be within [0,180].", value);
            }
            config.corner_threshold = value
        } else {
            panic!("Parser Error: Corner threshold is not numeric: {}.", value);
        }
    }

    if let Some(value) = matches.value_of("segment_length") {
        if value.trim().parse::<f64>().is_ok() {
            // is numeric
            let value = value.trim().parse::<f64>().unwrap();
            if value < 3.5 || value > 10.0 {
                panic!("Out of Range Error: Segment length is invalid at {}. It must be within [3.5,10].", value);
            }
            config.length_threshold = value;
        } else {
            panic!("Parser Error: Segment length is not numeric: {}.", value);
        }
    }

    if let Some(value) = matches.value_of("splice_threshold") {
        if value.trim().parse::<i32>().is_ok() {
            // is numeric
            let value = value.trim().parse::<i32>().unwrap();
            if value < 0 || value > 180 {
                panic!("Out of Range Error: Segment length is invalid at {}. It must be within [0,180].", value);
            }
            config.splice_threshold = value;
        } else {
            panic!("Parser Error: Segment length is not numeric: {}.", value);
        }
    }

    if let Some(value) = matches.value_of("path_precision") {
        if value.trim().parse::<u32>().is_ok() {
            // is numeric
            let value = value.trim().parse::<u32>().ok();
            config.path_precision = value;
        } else {
            panic!(
                "Parser Error: Path precision is not an unsigned integer: {}.",
                value
            );
        }
    }

    (input_path, output_path, config)
}

fn main() {
    let (input_path, output_path, config) = config_from_args();
    let result = converter::convert_image_to_svg(&input_path, &output_path, config);
    match result {
        Ok(()) => {
            println!("Conversion successful.");
        }
        Err(msg) => {
            panic!("Conversion failed with error message: {}", msg);
        }
    }
}

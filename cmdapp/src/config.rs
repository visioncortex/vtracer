use std::str::FromStr;
use std::path::PathBuf;
use clap::{Arg, App};
use visioncortex::PathSimplifyMode;

pub enum Preset {
    Bw,
    Poster,
    Photo
}

pub enum ColorMode {
    Color,
    Binary,
}

pub enum Hierarchical {
    Stacked,
    Cutout,
}

/// Converter config
pub struct Config {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub color_mode: ColorMode,
    pub hierarchical: Hierarchical,
    pub filter_speckle: usize,
    pub color_precision: i32,
    pub layer_difference: i32,
    pub mode: PathSimplifyMode,
    pub corner_threshold: i32,
    pub length_threshold: f64,
    pub max_iterations: usize,
    pub splice_threshold: i32,
    pub path_precision: Option<u32>,
}

pub(crate) struct ConverterConfig {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub color_mode: ColorMode,
    pub hierarchical: Hierarchical,
    pub filter_speckle_area: usize,
    pub color_precision_loss: i32,
    pub layer_difference: i32,
    pub mode: PathSimplifyMode,
    pub corner_threshold: f64,
    pub length_threshold: f64,
    pub max_iterations: usize,
    pub splice_threshold: f64,
    pub path_precision: Option<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            input_path: PathBuf::default(),
            output_path: PathBuf::default(),
            color_mode: ColorMode::Color,
            hierarchical: Hierarchical::Stacked,
            mode: PathSimplifyMode::Spline,
            filter_speckle: 4,
            color_precision: 6,
            layer_difference: 16,
            corner_threshold: 60,
            length_threshold: 4.0,
            splice_threshold: 45,
            max_iterations: 10,
            path_precision: Some(8),
        }
    }
}

impl FromStr for ColorMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "color" => Ok(Self::Color),
            "binary" => Ok(Self::Binary),
            _ => Err(format!("unknown ColorMode {}", s)),
        }
    }
}

impl FromStr for Hierarchical {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stacked" => Ok(Self::Stacked),
            "cutout" => Ok(Self::Cutout),
            _ => Err(format!("unknown Hierarchical {}", s)),
        }
    }
}

impl FromStr for Preset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "bw" => Ok(Self::Bw),
            "poster" => Ok(Self::Poster),
            "photo" => Ok(Self::Photo),
            _ => Err(format!("unknown Preset {}", s)),
        }
    }
}

fn path_simplify_mode_from_str(s: &str) -> PathSimplifyMode {
    match s {
        "polygon" => PathSimplifyMode::Polygon,
        "spline" => PathSimplifyMode::Spline,
        "none" => PathSimplifyMode::None,
        _ => panic!("unknown PathSimplifyMode {}", s),
    }
}

impl Config {
    pub fn from_args() -> Self {
        let app = App::new("visioncortex VTracer ".to_owned() + env!("CARGO_PKG_VERSION"))
            .about("A cmd app to convert images into vector graphics.");

        let app = app.arg(Arg::with_name("input")
            .long("input")
            .short("i")
            .takes_value(true)
            .help("Path to input raster image")
            .required(true));

        let app = app.arg(Arg::with_name("output")
            .long("output")
            .short("o")
            .takes_value(true)
            .help("Path to output vector graphics")
            .required(true));

        let app = app.arg(Arg::with_name("color_mode")
            .long("colormode")
            .takes_value(true)
            .help("True color image `color` (default) or Binary image `bw`"));

        let app = app.arg(Arg::with_name("hierarchical")
            .long("hierarchical")
            .takes_value(true)
            .help(
                "Hierarchical clustering `stacked` (default) or non-stacked `cutout`. \
                Only applies to color mode. "
            ));

        let app = app.arg(Arg::with_name("preset")
            .long("preset")
            .takes_value(true)
            .help("Use one of the preset configs `bw`, `poster`, `photo`"));

        let app = app.arg(Arg::with_name("filter_speckle")
            .long("filter_speckle")
            .short("f")
            .takes_value(true)
            .help("Discard patches smaller than X px in size"));

        let app = app.arg(Arg::with_name("color_precision")
            .long("color_precision")
            .short("p")
            .takes_value(true)
            .help("Number of significant bits to use in an RGB channel"));

        let app = app.arg(Arg::with_name("gradient_step")
            .long("gradient_step")
            .short("g")
            .takes_value(true)
            .help("Color difference between gradient layers"));

        let app = app.arg(Arg::with_name("corner_threshold")
            .long("corner_threshold")
            .short("c")
            .takes_value(true)
            .help("Minimum momentary angle (degree) to be considered a corner"));

        let app = app.arg(Arg::with_name("segment_length")
            .long("segment_length")
            .short("l")
            .takes_value(true)
            .help("Perform iterative subdivide smooth until all segments are shorter than this length"));

        let app = app.arg(Arg::with_name("splice_threshold")
            .long("splice_threshold")
            .short("s")
            .takes_value(true)
            .help("Minimum angle displacement (degree) to splice a spline"));

        let app = app.arg(Arg::with_name("mode")
            .long("mode")
            .short("m")
            .takes_value(true)
            .help("Curver fitting mode `pixel`, `polygon`, `spline`"));

        let app = app.arg(Arg::with_name("path_precision")
            .long("path_precision")
            .takes_value(true)
            .help("Number of decimal places to use in path string"));

        // Extract matches
        let matches = app.get_matches();

        let mut config = Config::default();
        let input_path = matches.value_of("input").expect("Input path is required, please specify it by --input or -i.");
        let output_path = matches.value_of("output").expect("Output path is required, please specify it by --output or -o.");

        if let Some(value) = matches.value_of("preset") {
            config = Self::from_preset(Preset::from_str(value).unwrap(), input_path, output_path);
        }

        config.input_path = PathBuf::from(input_path);
        config.output_path = PathBuf::from(output_path);

        if let Some(value) = matches.value_of("color_mode") {
            config.color_mode = ColorMode::from_str(if value.trim() == "bw" || value.trim() == "BW" {"binary"} else {"color"}).unwrap()
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
            if value.trim().parse::<usize>().is_ok() { // is numeric
                let value = value.trim().parse::<usize>().unwrap();
                if value > 16 {
                    panic!("Out of Range Error: Filter speckle is invalid at {}. It must be within [0,16].", value);
                }
                config.filter_speckle = value;
            } else {
                panic!("Parser Error: Filter speckle is not a positive integer: {}.", value);
            }
        }

        if let Some(value) = matches.value_of("color_precision") {
            if value.trim().parse::<i32>().is_ok() { // is numeric
                let value = value.trim().parse::<i32>().unwrap();
                if value < 1 || value > 8 {
                    panic!("Out of Range Error: Color precision is invalid at {}. It must be within [1,8].", value);
                }
                config.color_precision = value;
            } else {
                panic!("Parser Error: Color precision is not an integer: {}.", value);
            }
        }

        if let Some(value) = matches.value_of("gradient_step") {
            if value.trim().parse::<i32>().is_ok() { // is numeric
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
            if value.trim().parse::<i32>().is_ok() { // is numeric
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
            if value.trim().parse::<f64>().is_ok() { // is numeric
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
            if value.trim().parse::<i32>().is_ok() { // is numeric
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
            if value.trim().parse::<u32>().is_ok() { // is numeric
                let value = value.trim().parse::<u32>().ok();
                config.path_precision = value;
            } else {
                panic!("Parser Error: Path precision is not an unsigned integer: {}.", value);
            }
        }

        config
    }

    pub fn from_preset(preset: Preset, input_path: &str, output_path: &str) -> Self {
        let input_path = PathBuf::from(input_path);
        let output_path = PathBuf::from(output_path);
        match preset {
            Preset::Bw => Self {
                input_path,
                output_path,
                color_mode: ColorMode::Binary,
                hierarchical: Hierarchical::Stacked,
                filter_speckle: 4,
                color_precision: 6,
                layer_difference: 16,
                mode: PathSimplifyMode::Spline,
                corner_threshold: 60,
                length_threshold: 4.0,
                max_iterations: 10,
                splice_threshold: 45,
                path_precision: Some(8),
            },
            Preset::Poster => Self {
                input_path,
                output_path,
                color_mode: ColorMode::Color,
                hierarchical: Hierarchical::Stacked,
                filter_speckle: 4,
                color_precision: 8,
                layer_difference: 16,
                mode: PathSimplifyMode::Spline,
                corner_threshold: 60,
                length_threshold: 4.0,
                max_iterations: 10,
                splice_threshold: 45,
                path_precision: Some(8),
            },
            Preset::Photo => Self {
                input_path,
                output_path,
                color_mode: ColorMode::Color,
                hierarchical: Hierarchical::Stacked,
                filter_speckle: 10,
                color_precision: 8,
                layer_difference: 48,
                mode: PathSimplifyMode::Spline,
                corner_threshold: 180,
                length_threshold: 4.0,
                max_iterations: 10,
                splice_threshold: 45,
                path_precision: Some(8),
            }
        }
    }

    pub(crate) fn into_converter_config(self) -> ConverterConfig {
        ConverterConfig {
            input_path: self.input_path,
            output_path: self.output_path,
            color_mode: self.color_mode,
            hierarchical: self.hierarchical,
            filter_speckle_area: self.filter_speckle * self.filter_speckle,
            color_precision_loss: 8 - self.color_precision,
            layer_difference: self.layer_difference,
            mode: self.mode,
            corner_threshold: deg2rad(self.corner_threshold),
            length_threshold: self.length_threshold,
            max_iterations: self.max_iterations,
            splice_threshold: deg2rad(self.splice_threshold),
            path_precision: self.path_precision,
        }
    }
}

fn deg2rad(deg: i32) -> f64 {
    deg as f64 / 180.0 * std::f64::consts::PI
}
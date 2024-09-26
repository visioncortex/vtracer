use std::str::FromStr;
use visioncortex::PathSimplifyMode;

#[derive(Debug, Clone)]
pub enum Preset {
    Bw,
    Poster,
    Photo,
}

#[derive(Debug, Clone)]
pub enum ColorMode {
    Color,
    Binary,
}

#[derive(Debug, Clone)]
pub enum Hierarchical {
    Stacked,
    Cutout,
}

/// Converter config
#[derive(Debug, Clone)]
pub struct Config {
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

#[derive(Debug, Clone)]
pub(crate) struct ConverterConfig {
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
            path_precision: Some(2),
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

impl Config {
    pub fn from_preset(preset: Preset) -> Self {
        match preset {
            Preset::Bw => Self {
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
                path_precision: Some(2),
            },
            Preset::Poster => Self {
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
                path_precision: Some(2),
            },
            Preset::Photo => Self {
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
                path_precision: Some(2),
            },
        }
    }

    pub(crate) fn into_converter_config(self) -> ConverterConfig {
        ConverterConfig {
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

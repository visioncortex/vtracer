//! High-level configuration and presets that assemble a [`Pipeline`].

use std::str::FromStr;

use visioncortex::Color;

use crate::colorfit::{AutoQuantize, ColorFitter, FixedPalette, Identity, MergeAdjacent};
use crate::compose::Compositing;
use crate::error::Error;
use crate::fitter::{CurveFitter, FitParams, PixelFitter, PolygonFitter, SplineFitter};
use crate::frontend::{
    BinaryFrontend, ColorClusterFrontend, Frontend, Threshold, WatershedFrontend,
};
use crate::mosaic::{
    PixelSegmentFitter, PolygonSegmentFitter, SegmentFitter, SplineSegmentFitter,
};
use crate::optimize::{OptimizerPass, QuantizePass, SimplifyPass};
use crate::pipeline::Pipeline;
use crate::svg::SvgWriter;

/// Which region-forming algorithm segments the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clustering {
    /// Hierarchical color clustering — the classic VTracer path.
    ColorCluster,
    /// Threshold to black/white, then cluster the foreground.
    Binary,
    /// Hierarchical watershed on the pixel graph, cut at `watershed_detail`.
    Watershed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hierarchical {
    Stacked,
    /// True mosaic cutout — not yet implemented (separate milestone).
    Cutout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    Pixel,
    Polygon,
    Spline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Bw,
    Poster,
    Photo,
}

/// The clustering-relevant projection of a [`Config`]. Two configs with equal
/// keys produce the same [`Segmentation`](crate::Segmentation), so a cached one
/// stays valid — this is what [`Session`](crate::Session) compares to decide
/// whether to re-segment. Kept in sync with [`Config::frontend`] in one place.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentKey {
    clustering: Clustering,
    color_precision: i32,
    layer_difference: i32,
    filter_speckle: usize,
    binary_threshold: u8,
    binary_adaptive: bool,
    binary_adaptive_window: u32,
    binary_adaptive_t: f64,
    watershed_detail: u8,
}

/// High-level converter configuration. [`Config::build`] turns this into a
/// concrete [`Pipeline`].
#[derive(Debug, Clone)]
pub struct Config {
    /// Region-forming algorithm (see [`Clustering`]).
    pub clustering: Clustering,
    pub hierarchical: Hierarchical,
    /// Speckle filter given as a side length; the area threshold is its square.
    pub filter_speckle: usize,
    /// Significant bits per RGB channel (1..=8).
    pub color_precision: i32,
    /// Color difference between gradient layers.
    pub layer_difference: i32,
    pub mode: FitMode,
    /// Corner threshold in degrees.
    pub corner_threshold: i32,
    /// Segment length threshold in pixels.
    pub length_threshold: f64,
    pub max_iterations: usize,
    /// Splice threshold in degrees.
    pub splice_threshold: i32,
    /// Coordinate precision (decimal places) for output.
    pub path_precision: Option<u32>,
    /// Fixed palette (empty = none). Takes priority over `max_colors`.
    pub palette: Vec<Color>,
    /// Auto-quantize target color count (None = off).
    pub max_colors: Option<usize>,
    /// Optimization level: 0 = off, 1 = quantize+simplify, 2 = + shorthands/grouping.
    pub optimize: u8,
    /// Binary-mode fixed threshold (0..=255): foreground when grayscale
    /// intensity is below this. Ignored when `binary_adaptive` is set.
    pub binary_threshold: u8,
    /// Binary mode: use Bradley–Roth adaptive thresholding instead of the fixed
    /// cutoff (better for uneven lighting).
    pub binary_adaptive: bool,
    /// Adaptive window side length in pixels; 0 = auto (~1/8 of the shorter side).
    pub binary_adaptive_window: u32,
    /// Adaptive sensitivity `t`: percent below the local mean (default 15).
    pub binary_adaptive_t: f64,
    /// Watershed clustering: where to cut the hierarchy (0..=255). Higher
    /// keeps more regions; 0 collapses the image to a single region.
    pub watershed_detail: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            clustering: Clustering::ColorCluster,
            hierarchical: Hierarchical::Stacked,
            filter_speckle: 4,
            color_precision: 6,
            layer_difference: 16,
            mode: FitMode::Spline,
            corner_threshold: 60,
            length_threshold: 4.0,
            max_iterations: 10,
            splice_threshold: 45,
            path_precision: Some(2),
            palette: Vec::new(),
            max_colors: None,
            optimize: 1,
            binary_threshold: 128,
            binary_adaptive: false,
            binary_adaptive_window: 0,
            binary_adaptive_t: 15.0,
            watershed_detail: 128,
        }
    }
}

impl Config {
    pub fn from_preset(preset: Preset) -> Self {
        match preset {
            Preset::Bw => Self {
                clustering: Clustering::Binary,
                ..Self::default()
            },
            Preset::Poster => Self {
                color_precision: 8,
                ..Self::default()
            },
            Preset::Photo => Self {
                filter_speckle: 10,
                color_precision: 8,
                layer_difference: 48,
                corner_threshold: 180,
                ..Self::default()
            },
        }
    }

    fn fit_params(&self) -> FitParams {
        FitParams {
            corner_threshold: deg2rad(self.corner_threshold),
            length_threshold: self.length_threshold,
            max_iterations: self.max_iterations,
            splice_threshold: deg2rad(self.splice_threshold),
        }
    }

    fn frontend(&self) -> Box<dyn Frontend> {
        match self.clustering {
            Clustering::ColorCluster => Box::new(ColorClusterFrontend {
                color_precision_loss: 8 - self.color_precision,
                layer_difference: self.layer_difference,
                good_min_area: self.speckle_area(),
            }),
            Clustering::Binary => {
                let threshold = if self.binary_adaptive {
                    Threshold::Adaptive {
                        window: self.binary_adaptive_window,
                        t: self.binary_adaptive_t,
                    }
                } else {
                    Threshold::Fixed(self.binary_threshold)
                };
                Box::new(BinaryFrontend {
                    threshold,
                    diagonal: false,
                    min_area: self.speckle_area(),
                })
            }
            Clustering::Watershed => Box::new(WatershedFrontend {
                detail: self.watershed_detail,
                min_area: self.speckle_area(),
            }),
        }
    }

    /// Speckle filter area (px), fed to the frontend.
    pub(crate) fn speckle_area(&self) -> usize {
        self.filter_speckle * self.filter_speckle
    }

    fn color_fitters(&self) -> Vec<Box<dyn ColorFitter>> {
        if !self.palette.is_empty() {
            vec![
                Box::new(FixedPalette::new(self.palette.clone())),
                Box::new(MergeAdjacent),
            ]
        } else if let Some(max_colors) = self.max_colors {
            vec![Box::new(AutoQuantize { max_colors }), Box::new(MergeAdjacent)]
        } else {
            vec![Box::new(Identity)]
        }
    }

    fn fitter(&self) -> Box<dyn CurveFitter> {
        match self.mode {
            FitMode::Pixel => Box::new(PixelFitter),
            FitMode::Polygon => Box::new(PolygonFitter),
            FitMode::Spline => Box::new(SplineFitter::new(self.fit_params())),
        }
    }

    fn segment_fitter(&self) -> Box<dyn SegmentFitter> {
        match self.mode {
            FitMode::Pixel => Box::new(PixelSegmentFitter),
            FitMode::Polygon => Box::new(PolygonSegmentFitter::default()),
            FitMode::Spline => Box::new(SplineSegmentFitter {
                corner_threshold: deg2rad(self.corner_threshold),
                length_threshold: self.length_threshold,
                max_iterations: self.max_iterations,
                splice_threshold: deg2rad(self.splice_threshold),
                ..SplineSegmentFitter::default()
            }),
        }
    }

    fn optimizers(&self) -> Vec<Box<dyn OptimizerPass>> {
        if self.optimize == 0 {
            return Vec::new();
        }
        let precision = self.path_precision.unwrap_or(2);
        vec![
            Box::new(QuantizePass::new(precision)),
            Box::new(SimplifyPass),
        ]
    }

    fn writer(&self) -> SvgWriter {
        match self.optimize {
            0 => SvgWriter {
                relative: false,
                shorthands: false,
                precision: self.path_precision,
            },
            1 => SvgWriter {
                relative: true,
                shorthands: false,
                precision: self.path_precision,
            },
            _ => SvgWriter {
                relative: true,
                shorthands: true,
                precision: self.path_precision,
            },
        }
    }

    /// The clustering-relevant subset of this config. Changing any field it
    /// captures (clustering algorithm, color precision, layer difference,
    /// speckle, binary threshold settings, or watershed detail) requires
    /// re-segmenting; changing anything else — fit mode, curve params,
    /// compositing, palette, optimization — reuses a cached segmentation. See
    /// [`Session`](crate::Session).
    pub fn segment_key(&self) -> SegmentKey {
        SegmentKey {
            clustering: self.clustering,
            color_precision: self.color_precision,
            layer_difference: self.layer_difference,
            filter_speckle: self.filter_speckle,
            binary_threshold: self.binary_threshold,
            binary_adaptive: self.binary_adaptive,
            binary_adaptive_window: self.binary_adaptive_window,
            binary_adaptive_t: self.binary_adaptive_t,
            watershed_detail: self.watershed_detail,
        }
    }

    /// Assemble a concrete pipeline from this configuration.
    pub fn build(&self) -> Result<Pipeline, Error> {
        let compositing = match self.hierarchical {
            Hierarchical::Stacked => Compositing::Stacked(self.fitter()),
            Hierarchical::Cutout => Compositing::Mosaic {
                fitter: self.segment_fitter(),
                // Rejoin flattened neighbours the gradient layering split:
                // clustering itself considers colors within one gradient step
                // to be the same region (`deepen_diff`). The watershed
                // hierarchy already decided every merge, so its partition
                // keeps its shape — only identical-color neighbours (e.g.
                // after a palette snap) still collapse into one face.
                merge_diff: match self.clustering {
                    Clustering::Watershed => 0,
                    _ => self.layer_difference,
                },
            },
        };

        Ok(Pipeline {
            frontend: self.frontend(),
            color_fitters: self.color_fitters(),
            compositing,
            optimizers: self.optimizers(),
            writer: self.writer(),
        })
    }
}

fn deg2rad(deg: i32) -> f64 {
    deg as f64 / 180.0 * std::f64::consts::PI
}

impl FromStr for Clustering {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "color-cluster" | "colorcluster" | "color" => Ok(Self::ColorCluster),
            "binary" | "bw" | "BW" => Ok(Self::Binary),
            "watershed" => Ok(Self::Watershed),
            _ => Err(format!("unknown clustering {s}")),
        }
    }
}

impl FromStr for Hierarchical {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stacked" => Ok(Self::Stacked),
            "cutout" => Ok(Self::Cutout),
            _ => Err(format!("unknown hierarchical mode {s}")),
        }
    }
}

impl FromStr for FitMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pixel" | "none" => Ok(Self::Pixel),
            "polygon" => Ok(Self::Polygon),
            "spline" => Ok(Self::Spline),
            _ => Err(format!("unknown fit mode {s}")),
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
            _ => Err(format!("unknown preset {s}")),
        }
    }
}

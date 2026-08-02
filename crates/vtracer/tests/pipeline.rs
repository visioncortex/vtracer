//! End-to-end pipeline smoke tests over synthetic images.

use vtracer::{ColorImage, Clustering, Config, FitMode, Hierarchical};

/// Build a `size × size` image split into two vertical color bands.
fn two_band_image(size: usize) -> ColorImage {
    let mut pixels = Vec::with_capacity(size * size * 4);
    for _y in 0..size {
        for x in 0..size {
            let (r, g, b) = if x < size / 2 {
                (220, 40, 40)
            } else {
                (40, 40, 220)
            };
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    ColorImage {
        pixels,
        width: size,
        height: size,
    }
}

fn assert_valid_svg(svg: &str) {
    assert!(svg.contains("<svg"), "missing <svg> element:\n{svg}");
    assert!(svg.trim_end().ends_with("</svg>"), "missing </svg> close");
    assert!(svg.contains("<path"), "expected at least one path:\n{svg}");
}

#[test]
fn default_color_pipeline_produces_svg() {
    let img = two_band_image(32);
    let svg = Config::default().build().unwrap().to_svg(&img).unwrap();
    assert_valid_svg(&svg);
}

#[test]
fn all_fit_modes_produce_svg() {
    let img = two_band_image(32);
    for mode in [FitMode::Pixel, FitMode::Polygon, FitMode::Spline] {
        let config = Config {
            mode,
            ..Config::default()
        };
        let svg = config.build().unwrap().to_svg(&img).unwrap();
        assert_valid_svg(&svg);
    }
}

#[test]
fn binary_pipeline_produces_svg() {
    let img = two_band_image(32);
    let config = Config {
        clustering: Clustering::Binary,
        ..Config::default()
    };
    let svg = config.build().unwrap().to_svg(&img).unwrap();
    assert_valid_svg(&svg);
}

#[test]
fn watershed_pipeline_produces_svg() {
    let img = two_band_image(32);
    for hierarchical in [Hierarchical::Stacked, Hierarchical::Cutout] {
        let config = Config {
            clustering: Clustering::Watershed,
            hierarchical,
            ..Config::default()
        };
        let svg = config.build().unwrap().to_svg(&img).unwrap();
        assert_valid_svg(&svg);
    }
}

#[test]
fn optimize_levels_shrink_or_match() {
    let img = two_band_image(48);
    let mut sizes = Vec::new();
    for level in [0u8, 1, 2] {
        let config = Config {
            optimize: level,
            ..Config::default()
        };
        let svg = config.build().unwrap().to_svg(&img).unwrap();
        assert_valid_svg(&svg);
        sizes.push(svg.len());
    }
    // Higher optimization should never produce larger output than level 0.
    assert!(sizes[1] <= sizes[0], "opt1 {} > opt0 {}", sizes[1], sizes[0]);
    assert!(sizes[2] <= sizes[0], "opt2 {} > opt0 {}", sizes[2], sizes[0]);
}

#[test]
fn mosaic_cutout_produces_svg() {
    let img = two_band_image(32);
    let config = Config {
        hierarchical: Hierarchical::Cutout,
        ..Config::default()
    };
    let svg = config.build().unwrap().to_svg(&img).unwrap();
    assert_valid_svg(&svg);
}

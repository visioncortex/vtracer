//! Golden-snapshot tests that lock in the exact SVG output of the pipeline.
//!
//! Fixtures use synthetic, in-code images rather than the JPEG samples on
//! purpose: JPEG decoding is image-crate-version dependent (verified against
//! the retired 0.6.x cmdapp), so JPEG goldens would be fragile. Synthetic
//! images are fully deterministic and still exercise every stage — hierarchical
//! clustering, all three fitters, color fitting, the optimizer passes, and the
//! writer's encoding choices.
//!
//! Regenerate goldens after an intentional behavior change with:
//!
//! ```sh
//! VTRACER_BLESS=1 cargo test -p vtracer --test golden
//! ```

use std::path::PathBuf;

use vtracer::{Color, ColorImage, ColorMode, Config, FitMode};

// --- synthetic image builders ------------------------------------------------

fn mk<F: Fn(usize, usize) -> (u8, u8, u8, u8)>(w: usize, h: usize, f: F) -> ColorImage {
    let mut pixels = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let (r, g, b, a) = f(x, y);
            pixels.extend_from_slice(&[r, g, b, a]);
        }
    }
    ColorImage {
        pixels,
        width: w,
        height: h,
    }
}

/// Four vertical color bands.
fn bands() -> ColorImage {
    let cols = [
        (220, 40, 40),
        (40, 200, 60),
        (50, 60, 220),
        (230, 210, 40),
    ];
    mk(48, 40, |x, _| {
        let (r, g, b) = cols[(x * cols.len()) / 48];
        (r, g, b, 255)
    })
}

/// Checkerboard of 8x8 cells — exercises region adjacency and holes.
fn checker() -> ColorImage {
    mk(48, 48, |x, y| {
        if ((x / 8) + (y / 8)) % 2 == 0 {
            (20, 20, 20, 255)
        } else {
            (235, 235, 235, 255)
        }
    })
}

/// A filled disc on a contrasting background — exercises curve fitting.
fn disc() -> ColorImage {
    let (cx, cy, r2) = (24.0f64, 24.0f64, 16.0f64 * 16.0);
    mk(48, 48, |x, y| {
        let dx = x as f64 - cx;
        let dy = y as f64 - cy;
        if dx * dx + dy * dy <= r2 {
            (200, 60, 60, 255)
        } else {
            (240, 240, 240, 255)
        }
    })
}

/// An annulus (disc with a hole) — exercises hole tracing.
fn ring() -> ColorImage {
    let (cx, cy) = (24.0f64, 24.0f64);
    mk(48, 48, |x, y| {
        let dx = x as f64 - cx;
        let dy = y as f64 - cy;
        let d2 = dx * dx + dy * dy;
        if d2 <= 20.0 * 20.0 && d2 >= 9.0 * 9.0 {
            (40, 90, 200, 255)
        } else {
            (245, 245, 245, 255)
        }
    })
}

/// A 4x4 grid of 16 distinct saturated colors — produces many hierarchical
/// layers, and gives auto-quantize something real to reduce.
fn swatches() -> ColorImage {
    let step = [0u8, 85, 170, 255];
    mk(48, 48, |x, y| {
        let col = (x / 12).min(3);
        let row = (y / 12).min(3);
        (step[col], step[row], 128, 255)
    })
}

// --- fixture matrix ----------------------------------------------------------

fn base() -> Config {
    Config::default()
}

fn cases() -> Vec<(&'static str, ColorImage, Config)> {
    vec![
        // Fit modes on the same content.
        ("bands_spline", bands(), base()),
        (
            "bands_polygon",
            bands(),
            Config {
                mode: FitMode::Polygon,
                ..base()
            },
        ),
        (
            "bands_pixel",
            bands(),
            Config {
                mode: FitMode::Pixel,
                optimize: 0,
                ..base()
            },
        ),
        // Curves and holes.
        ("disc_spline", disc(), base()),
        ("ring_spline", ring(), base()),
        ("checker_spline", checker(), base()),
        // Hierarchical layering.
        ("swatches_color", swatches(), base()),
        // Binary mode.
        (
            "checker_bw",
            checker(),
            Config {
                color_mode: ColorMode::Binary,
                ..base()
            },
        ),
        // Color fitting: fixed palette (+ merge) and auto-quantize (+ merge).
        (
            "bands_palette",
            bands(),
            Config {
                palette: vec![Color::new(0, 0, 0), Color::new(255, 255, 255)],
                optimize: 2,
                ..base()
            },
        ),
        (
            "swatches_quant4",
            swatches(),
            Config {
                max_colors: Some(4),
                optimize: 2,
                ..base()
            },
        ),
        // Optimizer / writer encoding levels on identical geometry.
        (
            "disc_opt0",
            disc(),
            Config {
                optimize: 0,
                ..base()
            },
        ),
        (
            "disc_opt2",
            disc(),
            Config {
                optimize: 2,
                ..base()
            },
        ),
    ]
}

fn goldens_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
}

#[test]
fn golden_snapshots() {
    let bless = std::env::var_os("VTRACER_BLESS").is_some();
    let dir = goldens_dir();
    if bless {
        std::fs::create_dir_all(&dir).unwrap();
    }

    let mut mismatches = Vec::new();
    for (name, img, config) in cases() {
        let svg = config
            .build()
            .unwrap_or_else(|e| panic!("case {name}: build failed: {e}"))
            .to_svg(&img)
            .unwrap_or_else(|e| panic!("case {name}: convert failed: {e}"));

        let path = dir.join(format!("{name}.svg"));
        if bless {
            std::fs::write(&path, &svg).unwrap();
            continue;
        }

        match std::fs::read_to_string(&path) {
            Ok(expected) if expected == svg => {}
            Ok(_) => mismatches.push(format!("{name}: output differs from golden")),
            Err(_) => mismatches.push(format!(
                "{name}: missing golden ({}); run with VTRACER_BLESS=1",
                path.display()
            )),
        }
    }

    assert!(
        mismatches.is_empty(),
        "golden mismatches:\n{}",
        mismatches.join("\n")
    );
}

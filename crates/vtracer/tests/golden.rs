//! Golden-snapshot tests over synthetic images, exercising every stage —
//! hierarchical clustering, all three fitters, color fitting, the optimizer
//! passes, and the writer.
//!
//! Goldens are compared by **rendering** both the stored SVG and the freshly
//! produced SVG and diffing pixels, not by byte-equality. The spline fitter's
//! cubic fit is floating-point, and f64 results differ by a few ULPs across
//! architectures (arm64 vs x86_64); after rounding, a coordinate can flip and
//! change the SVG bytes without any real geometry change. A visual diff is
//! encoding-agnostic and tolerant of that sub-pixel noise while still catching
//! genuine regressions.
//!
//! Regenerate goldens after an intentional behavior change with:
//!
//! ```sh
//! VTRACER_BLESS=1 cargo test -p vtracer --test golden
//! ```

use std::path::PathBuf;

use resvg::{tiny_skia, usvg};
use vtracer::{Color, ColorImage, ColorMode, Config, FitMode, Hierarchical};

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
        // Mosaic (seam-free tessellation): exact pixel and polygon fitters.
        (
            "disc_mosaic_pixel",
            disc(),
            Config {
                hierarchical: Hierarchical::Cutout,
                mode: FitMode::Pixel,
                ..base()
            },
        ),
        (
            "checker_mosaic_polygon",
            checker(),
            Config {
                hierarchical: Hierarchical::Cutout,
                mode: FitMode::Polygon,
                optimize: 2,
                ..base()
            },
        ),
        (
            "disc_mosaic_spline",
            disc(),
            Config {
                hierarchical: Hierarchical::Cutout,
                mode: FitMode::Spline,
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
            Ok(expected) => {
                if let Some(diff) = render_diff(&expected, &svg) {
                    mismatches.push(format!("{name}: {diff}"));
                }
            }
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

/// Render an SVG string to an RGBA pixmap at its intrinsic size.
fn render(svg: &str) -> (u32, u32, Vec<u8>) {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("parse golden svg");
    let size = tree.size();
    let (w, h) = (size.width().ceil() as u32, size.height().ceil() as u32);
    let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1)).expect("alloc pixmap");
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    (w, h, pixmap.data().to_vec())
}

/// Compare two SVGs by rendering. Returns `Some(reason)` if they differ beyond
/// a small tolerance (which absorbs cross-architecture sub-pixel float noise),
/// or `None` if visually equivalent.
fn render_diff(expected: &str, actual: &str) -> Option<String> {
    let (ew, eh, a) = render(expected);
    let (aw, ah, b) = render(actual);
    if (ew, eh) != (aw, ah) {
        return Some(format!("size {ew}x{eh} vs {aw}x{ah}"));
    }
    // A pixel "differs" only on a clear color change, not antialiasing wobble.
    const CHANNEL: u8 = 40;
    let total = (ew * eh) as usize;
    let differing = (0..total)
        .filter(|&p| (0..3).any(|c| a[p * 4 + c].abs_diff(b[p * 4 + c]) > CHANNEL))
        .count();
    // Allow a tiny fraction for boundary pixels that flip under sub-pixel shifts.
    let allowed = (total / 200).max(8); // 0.5%, min 8px
    if differing > allowed {
        Some(format!(
            "{differing}/{total} pixels differ (> {allowed} allowed) — real change, re-bless if intended"
        ))
    } else {
        None
    }
}

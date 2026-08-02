//! Two-phase pipeline: cache the expensive segmentation, re-run the cheap
//! downstream stages with different parameters (the interactive tuning loop).

use vtracer::{ColorImage, Config, FitMode};

/// A few colored blocks — several clusters, a few holes.
fn blocks() -> ColorImage {
    let (w, h) = (48usize, 48usize);
    let mut pixels = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let c = match (x / 16, y / 16) {
                (0, _) => (220u8, 40, 40),
                (1, 0) => (40, 200, 60),
                (1, _) => (50, 60, 220),
                _ => (230, 210, 40),
            };
            pixels.extend_from_slice(&[c.0, c.1, c.2, 255]);
        }
    }
    ColorImage {
        pixels,
        width: w,
        height: h,
    }
}

fn cfg(mode: FitMode) -> Config {
    Config {
        mode,
        ..Config::default()
    }
}

/// `finish(segment(img))` equals the one-shot `run(img)`.
#[test]
fn two_phase_matches_one_shot() {
    let img = blocks();
    let pipeline = cfg(FitMode::Spline).build().unwrap();

    let one_shot = pipeline.run(&img).unwrap();
    let seg = pipeline.segment(&img).unwrap();
    let two_phase = pipeline.finish(&seg).unwrap();

    assert_eq!(
        pipeline.writer.write(&one_shot),
        pipeline.writer.write(&two_phase),
        "splitting segment/finish must not change the output"
    );
}

/// A cached segmentation stays pristine — `finish` can be called repeatedly and
/// deterministically (color fitting mutates only an internal clone).
#[test]
fn cached_segmentation_is_reusable() {
    let img = blocks();
    let pipeline = cfg(FitMode::Polygon).build().unwrap();

    let seg = pipeline.segment(&img).unwrap();
    let first = pipeline.writer.write(&pipeline.finish(&seg).unwrap());
    let second = pipeline.writer.write(&pipeline.finish(&seg).unwrap());

    assert_eq!(first, second, "reusing a cached segmentation must be stable");
}

/// The tuning workflow: segment once, then feed that segmentation to pipelines
/// with different curve-fitting parameters. Same regions, different geometry —
/// and no re-segmentation. (Speckle, color precision, and layer difference are
/// clustering parameters, so changing them requires a fresh `segment`.)
#[test]
fn tune_curve_fitting_on_cached_segmentation() {
    let img = blocks();

    // Same clustering parameters (defaults), different fit modes → the
    // segmentation from one is valid input to the other's `finish`.
    let pixel = cfg(FitMode::Pixel).build().unwrap();
    let spline = cfg(FitMode::Spline).build().unwrap();

    let seg = pixel.segment(&img).unwrap();

    let doc_pixel = pixel.finish(&seg).unwrap();
    let doc_spline = spline.finish(&seg).unwrap();

    // Same partition → same number of shapes.
    assert_eq!(doc_pixel.shapes.len(), doc_spline.shapes.len());
    assert!(!doc_pixel.shapes.is_empty());

    // But the fitted geometry differs (straight edges vs cubic curves).
    assert_ne!(
        pixel.writer.write(&doc_pixel),
        spline.writer.write(&doc_spline),
        "pixel and spline fitting should produce different paths"
    );
}

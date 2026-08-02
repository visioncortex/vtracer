//! The curve-simplification stage, end to end: `Config::simplify` must cut
//! anchor counts in both compositing modes without changing geometry kind,
//! and leave output untouched when off (the goldens enforce the byte-level
//! version of that).

use vtracer::ir::PathCmd;
use vtracer::{ColorImage, Config, FitMode, Hierarchical, VectorDoc};

/// A filled disc — one long smooth boundary, the best case for merging the
/// per-splice cubics the spline fitter emits.
fn disc_image(size: usize) -> ColorImage {
    let mut pixels = Vec::with_capacity(size * size * 4);
    let (c, r) = (size as f64 / 2.0, size as f64 * 0.4);
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f64 + 0.5 - c, y as f64 + 0.5 - c);
            let (rr, gg, bb) = if (dx * dx + dy * dy).sqrt() < r {
                (200, 60, 60)
            } else {
                (240, 240, 240)
            };
            pixels.extend_from_slice(&[rr, gg, bb, 255]);
        }
    }
    ColorImage {
        pixels,
        width: size,
        height: size,
    }
}

fn cubic_count(doc: &VectorDoc) -> usize {
    doc.shapes
        .iter()
        .flat_map(|s| &s.path.subpaths)
        .flat_map(|sub| &sub.commands)
        .filter(|c| matches!(c, PathCmd::CubicTo(..)))
        .count()
}

fn run(config: &Config) -> VectorDoc {
    config.build().unwrap().run(&disc_image(128)).unwrap()
}

#[test]
fn simplify_reduces_cubics_in_stacked_mode() {
    let base = Config::default();
    let simplified = Config {
        simplify: Some(2.0),
        ..Config::default()
    };
    let (before, after) = (cubic_count(&run(&base)), cubic_count(&run(&simplified)));
    assert!(before > 0, "the disc must be traced with cubics");
    assert!(
        after < before,
        "simplify must reduce anchors: {before} -> {after}"
    );
}

#[test]
fn simplify_reduces_cubics_in_cutout_mode() {
    let cutout = |simplify| Config {
        hierarchical: Hierarchical::Cutout,
        simplify,
        ..Config::default()
    };
    let (before, after) = (
        cubic_count(&run(&cutout(None))),
        cubic_count(&run(&cutout(Some(2.0)))),
    );
    assert!(before > 0, "the disc must be traced with cubics");
    assert!(
        after < before,
        "simplify must reduce anchors: {before} -> {after}"
    );
}

#[test]
fn simplify_leaves_polyline_modes_untouched() {
    for mode in [FitMode::Pixel, FitMode::Polygon] {
        let base = Config {
            mode,
            ..Config::default()
        };
        let simplified = Config {
            simplify: Some(2.0),
            ..base.clone()
        };
        let a = base.build().unwrap().to_svg(&disc_image(64)).unwrap();
        let b = simplified.build().unwrap().to_svg(&disc_image(64)).unwrap();
        assert_eq!(a, b, "{mode:?} output must not change");
    }
}

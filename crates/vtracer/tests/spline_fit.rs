//! Spline fitting stays anchored to the geometry it approximates.
//!
//! Regression for the sparse-slice ballooning bug: a splice slice with very
//! uneven point spacing (a few-pixel jog then a long straight leg, produced by
//! the walker around thin strands) used to be fitted by a single cubic that
//! interpolated the samples exactly while swinging ~30 px sideways between
//! them — its control points landing far outside the shape itself. The
//! Cityscape sample at color precision 8 / gradient step 28 is the real
//! reproduction (a 1 px, 330 px-tall strand in the maroon region).

use std::path::PathBuf;

use vtracer::ir::PathCmd;
use vtracer::{ColorImage, Config, Hierarchical, VectorDoc};

fn cityscape() -> ColorImage {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../docs/assets/samples/Cityscape Sunset_DFM3-01.jpg");
    let img = image::open(&p).expect("sample image").to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    ColorImage {
        pixels: img.into_raw(),
        width: w,
        height: h,
    }
}

/// Every cubic's control points must stay within its shape's on-curve bounding
/// box plus a small overshoot allowance. The ballooning bug put handles ~25 px
/// outside the whole shape; a healthy fit stays within the fit error (10).
fn assert_handles_anchored(doc: &VectorDoc, margin: f64) {
    for (si, shape) in doc.shapes.iter().enumerate() {
        // Bounding box over on-curve points only.
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        let mut on_curve = |p: &visioncortex::PointF64| {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        };
        for sub in &shape.path.subpaths {
            for cmd in &sub.commands {
                match cmd {
                    PathCmd::MoveTo(p) | PathCmd::LineTo(p) => on_curve(p),
                    PathCmd::CubicTo(_, _, p) => on_curve(p),
                    PathCmd::Close => {}
                }
            }
        }
        for sub in &shape.path.subpaths {
            for cmd in &sub.commands {
                if let PathCmd::CubicTo(c1, c2, _) = cmd {
                    for q in [c1, c2] {
                        assert!(
                            q.x >= x0 - margin
                                && q.x <= x1 + margin
                                && q.y >= y0 - margin
                                && q.y <= y1 + margin,
                            "shape {si}: control point ({},{}) strays outside \
                             bbox ({x0},{y0})..({x1},{y1}) + {margin}",
                            q.x,
                            q.y
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn spline_handles_stay_anchored_on_photo() {
    let img = cityscape();
    let base = Config {
        color_precision: 8,
        layer_difference: 28,
        ..Config::default()
    };

    // Stacked: per-region closed outlines through Spline::from_path_f64.
    let doc = base.build().unwrap().run(&img).unwrap();
    assert!(doc.shapes.len() > 500, "sanity: the trace produced real output");
    assert_handles_anchored(&doc, 15.0);

    // Cutout: open boundary segments through the mosaic's segment fitter.
    let cutout = Config {
        hierarchical: Hierarchical::Cutout,
        ..base
    };
    let doc = cutout.build().unwrap().run(&img).unwrap();
    assert_handles_anchored(&doc, 15.0);
}

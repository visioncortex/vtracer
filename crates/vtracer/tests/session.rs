//! `Session` caches the segmentation and re-segments only when a clustering
//! parameter changes — verified both at the key level and end-to-end.

use visioncortex::Color;
use vtracer::{
    CancelToken, Clustering, ColorImage, Config, FitMode, Hierarchical, Session,
};

/// A few colored blocks — several clusters.
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

/// The key partition: finish-phase params share a segment key; clustering
/// params change it. This is the contract `Session` relies on.
#[test]
fn segment_key_tracks_only_clustering_params() {
    let base = Config::default();

    // Finish-phase changes → same key (segmentation is reusable).
    for tweaked in [
        Config {
            corner_threshold: 90,
            ..base.clone()
        },
        Config {
            optimize: 0,
            ..base.clone()
        },
        Config {
            hierarchical: vtracer::Hierarchical::Cutout,
            ..base.clone()
        },
        Config {
            max_colors: Some(4),
            ..base.clone()
        },
    ] {
        assert_eq!(
            base.segment_key(),
            tweaked.segment_key(),
            "finish-phase param must not change the segment key"
        );
    }

    // Clustering changes → different key (must re-segment).
    for tweaked in [
        Config {
            filter_speckle: base.filter_speckle + 4,
            ..base.clone()
        },
        Config {
            color_precision: 4,
            ..base.clone()
        },
        Config {
            layer_difference: 32,
            ..base.clone()
        },
        Config {
            clustering: vtracer::Clustering::Binary,
            ..base.clone()
        },
        Config {
            clustering: vtracer::Clustering::Watershed,
            ..base.clone()
        },
        Config {
            watershed_detail: 200,
            ..base.clone()
        },
    ] {
        assert_ne!(
            base.segment_key(),
            tweaked.segment_key(),
            "clustering param must change the segment key"
        );
    }
}

/// A `Session` render equals the one-shot pipeline — for a finish-only change
/// (reuses the cache) and for a clustering change (re-segments). Correctness is
/// identical either way; the cache is a transparent optimization.
#[test]
fn session_matches_one_shot() {
    let img = blocks();
    let mut session = Session::new(img.clone());

    let base = Config::default();
    let svg0 = session.render_svg(&base).unwrap();
    assert_eq!(
        svg0,
        base.build().unwrap().to_svg(&img).unwrap(),
        "first render must match the one-shot pipeline"
    );

    // Finish-only change: reuses the cached segmentation.
    let tuned = Config {
        corner_threshold: 90,
        ..base.clone()
    };
    assert_eq!(
        session.render_svg(&tuned).unwrap(),
        tuned.build().unwrap().to_svg(&img).unwrap(),
        "reused-segmentation render must match the one-shot pipeline"
    );

    // Clustering change: re-segments, still matches the one-shot.
    let respeckled = Config {
        filter_speckle: base.filter_speckle + 4,
        ..base.clone()
    };
    assert_eq!(
        session.render_svg(&respeckled).unwrap(),
        respeckled.build().unwrap().to_svg(&img).unwrap(),
        "re-segmented render must match the one-shot pipeline"
    );
}

/// Blocks plus a gradient band and a small fleck — structure that makes every
/// clustering parameter (speckle, precision, gradient step, watershed detail,
/// thresholds) actually change the output.
fn textured() -> ColorImage {
    let (w, h) = (48usize, 48usize);
    let mut pixels = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let c = if y >= 32 {
                let g = 60 + (x * 3) as u8; // gradient band
                (g, g, 200)
            } else if (4..7).contains(&x) && (4..7).contains(&y) {
                (10, 200, 10) // 9 px fleck
            } else {
                match (x / 16, y / 16) {
                    (0, _) => (220u8, 40, 40),
                    (1, _) => (40, 200, 60),
                    _ => (230, 210, 40),
                }
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

/// The exhaustive contract: walk a cumulative sequence of config changes that
/// touches every parameter category — finish-phase dials, clustering dials,
/// frontend switches (including leaving watershed and coming back to its
/// cached hierarchy), compositing, palettes — and after each step the cached
/// session render must be byte-identical to a from-scratch one-shot pipeline.
#[test]
fn session_equals_one_shot_across_param_walk() {
    let img = textured();
    let mut session = Session::new(img.clone());
    let mut cfg = Config::default();

    let steps: Vec<(&str, fn(&mut Config))> = vec![
        ("initial", |_| {}),
        // Finish-phase changes (cache hits).
        ("corner_threshold", |c| c.corner_threshold = 90),
        ("mode polygon", |c| c.mode = FitMode::Polygon),
        ("optimize 2", |c| c.optimize = 2),
        ("cutout", |c| c.hierarchical = Hierarchical::Cutout),
        ("path_precision", |c| c.path_precision = Some(1)),
        // Clustering changes (re-segment).
        ("filter_speckle", |c| c.filter_speckle = 6),
        ("layer_difference", |c| c.layer_difference = 32),
        ("color_precision", |c| c.color_precision = 5),
        // Watershed, incl. cheap re-cuts of the cached hierarchy.
        ("watershed", |c| c.clustering = Clustering::Watershed),
        ("detail 200", |c| c.watershed_detail = 200),
        ("detail 64", |c| c.watershed_detail = 64),
        ("stacked", |c| c.hierarchical = Hierarchical::Stacked),
        ("mode spline", |c| c.mode = FitMode::Spline),
        // Binary, with both thresholding methods.
        ("binary", |c| c.clustering = Clustering::Binary),
        ("threshold 100", |c| c.binary_threshold = 100),
        ("adaptive", |c| c.binary_adaptive = true),
        // Back to watershed: the hierarchy cache must still be valid.
        ("watershed again", |c| c.clustering = Clustering::Watershed),
        ("quantize", |c| c.max_colors = Some(4)),
        // And back to the color path with a palette.
        ("color-cluster", |c| {
            c.clustering = Clustering::ColorCluster;
            c.max_colors = None;
            c.palette = vec![
                Color::new(0, 0, 0),
                Color::new(255, 255, 255),
                Color::new(200, 40, 40),
            ];
        }),
        ("speckle again", |c| c.filter_speckle = 2),
    ];

    for (name, step) in steps {
        step(&mut cfg);
        assert_eq!(
            session.render_svg(&cfg).unwrap(),
            cfg.build().unwrap().to_svg(&img).unwrap(),
            "step `{name}`: cached session render must equal a full rebuild"
        );
    }
}

/// The progress-reporting render path (which segments through a different
/// branch, including the watershed hierarchy shortcut) produces the same
/// document as the plain path and the one-shot pipeline.
#[test]
fn render_with_progress_matches_plain_render() {
    let img = textured();
    for clustering in [
        Clustering::ColorCluster,
        Clustering::Watershed,
        Clustering::Binary,
    ] {
        let cfg = Config {
            clustering,
            ..Config::default()
        };
        let one_shot = cfg.build().unwrap().to_svg(&img).unwrap();

        // Fresh session per variant so the progress path does the segmenting.
        let mut session = Session::new(img.clone());
        let doc = session
            .render_with_progress(&cfg, &CancelToken::new(), &mut |_| {})
            .unwrap();
        let progress_svg = cfg.build().unwrap().writer.write(&doc);
        assert_eq!(
            progress_svg, one_shot,
            "{clustering:?}: progress path must equal the one-shot pipeline"
        );

        // And the now-warm cache serves the plain path identically.
        assert_eq!(
            session.render_svg(&cfg).unwrap(),
            one_shot,
            "{clustering:?}: cache warmed by the progress path must match too"
        );
    }
}

/// `invalidate` drops all cached state; the next render rebuilds from scratch
/// and still matches.
#[test]
fn invalidate_then_render_matches() {
    let img = textured();
    let cfg = Config {
        clustering: Clustering::Watershed,
        ..Config::default()
    };
    let one_shot = cfg.build().unwrap().to_svg(&img).unwrap();

    let mut session = Session::new(img);
    assert_eq!(session.render_svg(&cfg).unwrap(), one_shot);
    session.invalidate();
    assert_eq!(
        session.render_svg(&cfg).unwrap(),
        one_shot,
        "render after invalidate must rebuild identically"
    );
}

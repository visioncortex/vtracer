//! `Session` caches the segmentation and re-segments only when a clustering
//! parameter changes — verified both at the key level and end-to-end.

use vtracer::{ColorImage, Config, Session};

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

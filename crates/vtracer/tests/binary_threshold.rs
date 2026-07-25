//! Binary thresholding: tunable fixed cutoff and Bradley–Roth adaptive.

use vtracer::frontend::{BinaryFrontend, Frontend};
use vtracer::{ColorImage, Threshold};

fn gray(w: usize, h: usize, f: impl Fn(usize, usize) -> u8) -> ColorImage {
    let mut pixels = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let v = f(x, y);
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    ColorImage {
        pixels,
        width: w,
        height: h,
    }
}

/// Total foreground pixels selected by a frontend over an image.
fn foreground_area(front: &BinaryFrontend, img: &ColorImage) -> usize {
    front
        .segment(img)
        .unwrap()
        .layers
        .iter()
        .map(|l| l.mask.area())
        .sum()
}

/// A uniform gray field: a lower fixed threshold selects strictly fewer pixels.
#[test]
fn fixed_threshold_is_tunable() {
    // Left third value 80, middle 130, right 180.
    let img = gray(60, 20, |x, _| match x / 20 {
        0 => 80,
        1 => 130,
        _ => 180,
    });

    let front = |v: u8| BinaryFrontend {
        threshold: Threshold::Fixed(v),
        diagonal: false,
        min_area: 0,
    };

    let low = foreground_area(&front(100), &img); // catches only the 80 band
    let mid = foreground_area(&front(150), &img); // 80 + 130 bands
    let high = foreground_area(&front(200), &img); // everything

    assert!(
        low < mid && mid < high,
        "higher threshold must select more foreground: {low} < {mid} < {high}"
    );
    assert_eq!(high, 60 * 20, "threshold above all values selects everything");
}

/// Adaptive thresholding recovers locally-dark marks under a brightness
/// gradient that no single global cutoff can separate.
#[test]
fn adaptive_beats_fixed_under_uneven_lighting() {
    let (w, h) = (80, 40);

    // Background ramps left(70) → right(210). Two 6x6 marks, each 40 darker
    // than their local background: one on the dark side, one on the bright side.
    let bg = |x: usize| 70 + (x * 140 / (w - 1)) as u8;
    let marks = [(16usize, 17usize), (60, 17)];
    let is_mark = |x: usize, y: usize| {
        marks
            .iter()
            .any(|&(mx, my)| x >= mx && x < mx + 6 && y >= my && y < my + 6)
    };
    let img = gray(w, h, |x, y| {
        if is_mark(x, y) {
            bg(x).saturating_sub(40)
        } else {
            bg(x)
        }
    });

    let base = BinaryFrontend {
        threshold: Threshold::Fixed(128),
        diagonal: false,
        min_area: 4,
    };

    // A global cutoff can't isolate both marks: 128 catches the dark-side mark
    // but floods the whole dark half of the ramp, and misses the bright-side
    // mark (~136) entirely — so fixed has no region on the bright half.
    let fixed_seg = base.segment(&img).unwrap();
    let fixed_area: usize = fixed_seg.layers.iter().map(|l| l.mask.area()).sum();
    let mid = (w as i32) / 2;
    let fixed_right = fixed_seg.layers.iter().any(|l| l.mask.offset.x >= mid);

    // Adaptive: window comfortably larger than the 6px marks so they fill.
    let adaptive = BinaryFrontend {
        threshold: Threshold::Adaptive {
            window: 21,
            t: 15.0,
        },
        ..base.clone()
    };
    let adaptive_seg = adaptive.segment(&img).unwrap();
    let adaptive_area: usize = adaptive_seg.layers.iter().map(|l| l.mask.area()).sum();
    let adaptive_left = adaptive_seg.layers.iter().any(|l| l.mask.offset.x < mid);
    let adaptive_right = adaptive_seg.layers.iter().any(|l| l.mask.offset.x >= mid);

    // The point of adaptive: it finds locally-dark marks on *both* sides of the
    // ramp, where the global threshold catches only the dark half.
    assert!(!fixed_right, "fixed(128) should miss the bright-side mark");
    assert!(
        adaptive_left && adaptive_right,
        "adaptive should detect marks on both the dark and bright sides"
    );
    assert!(adaptive_area > 0, "adaptive must select some foreground");
    assert!(
        adaptive_area * 3 < fixed_area,
        "adaptive should select far less than fixed's flooded half: \
         adaptive={adaptive_area}, fixed={fixed_area}"
    );
}

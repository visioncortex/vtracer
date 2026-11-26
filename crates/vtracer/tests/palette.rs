//! Automatic palette selection ([`AutoQuantize`]) on synthetic controls with
//! known answers: anti-aliased blends never take a slot, solid fills do, and a
//! thin mark's ink is admitted only once it covers 1% of the image.

use vtracer::colorfit::AutoQuantize;
use vtracer::{ColorImage, Config, Segmentation};

type Rgb = [u8; 3];

const RED: Rgb = [255, 0, 0];
const BLUE: Rgb = [0, 0, 255];
const PURPLE: Rgb = [128, 0, 128];

/// Segment a `w`×`h` image, keeping even single-pixel AA regions so the
/// controls exercise the palette selector rather than the clustering.
fn segment(w: usize, h: usize, rgb: impl Fn(usize, usize) -> Rgb) -> Segmentation {
    let mut pixels = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let [r, g, b] = rgb(x, y);
            pixels.extend([r, g, b, 255]);
        }
    }
    let config = Config {
        color_precision: 8,
        filter_speckle: 0,
        layer_difference: 0,
        ..Config::default()
    };
    config.build().unwrap().segment(&ColorImage { width: w, height: h, pixels }).unwrap()
}

fn palette(seg: &Segmentation, k: usize) -> Vec<Rgb> {
    AutoQuantize { max_colors: k }.palette(seg).iter().map(|c| c.rgb_u8()).collect()
}

fn assert_same_set(actual: &[Rgb], expected: &[Rgb]) {
    assert!(
        actual.len() == expected.len() && expected.iter().all(|c| actual.contains(c)),
        "palette {actual:?}, expected {expected:?}"
    );
}

/// Red and blue across an oblique anti-aliased edge, with an optional purple
/// rectangle `accent_w`×`accent_h` at (160, 80).
fn edge(accent_w: usize, accent_h: usize) -> Segmentation {
    segment(320, 192, |x, y| {
        if (160..160 + accent_w).contains(&x) && (80..80 + accent_h).contains(&y) {
            return PURPLE;
        }
        let red = ((116. + y as f64 / 16. - x as f64).clamp(0., 1.) * 255.).round() as u8;
        [red, 0, 255 - red]
    })
}

/// `count` slanted 64×1 px black bars on white, shaded by 16×16 supersampled
/// coverage, so every edge pixel is a gray AA blend. Each bar inks ~64 px.
fn slanted_ink(count: usize) -> Segmentation {
    let (sin, cos) = 20_f64.to_radians().sin_cos();
    segment(320, 192, |x, y| {
        if !(127..=193).contains(&x) {
            return [255; 3];
        }
        let mut covered = 0;
        for line in 0..count {
            let (cx, cy) = (160., (28 + line * 12) as f64);
            if (y as f64 - cy).abs() > 13. {
                continue;
            }
            for sy in 0..16 {
                for sx in 0..16 {
                    let dx = x as f64 + (sx as f64 + 0.5) / 16. - cx;
                    let dy = y as f64 + (sy as f64 + 0.5) / 16. - cy;
                    if (dx * cos + dy * sin).abs() <= 32. && (dy * cos - dx * sin).abs() <= 0.5 {
                        covered += 1;
                    }
                }
            }
        }
        [(255. * (1. - covered.min(256) as f64 / 256.)).round() as u8; 3]
    })
}

/// Five broad warm fills on white, crossed by a 1 px black diamond outline
/// rasterized by 16×16 supersampled coverage.
fn outlined_fills() -> Segmentation {
    segment(320, 128, |x, y| {
        if !(8..312).contains(&x) || !(8..120).contains(&y) {
            return [255; 3];
        }
        let fill: Rgb = match x {
            ..=92 => [241, 200, 142],
            93..=162 => [233, 165, 104],
            163..=222 => [215, 131, 83],
            223..=271 => [198, 101, 67],
            _ => [171, 77, 56],
        };
        // Distance to the diamond, in px.
        let boundary = |px: f64, py: f64| {
            ((px - 160.).abs() / 140. + (py - 64.).abs() / 43. - 1.).abs()
                / (1. / 140_f64.powi(2) + 1. / 43_f64.powi(2)).sqrt()
        };
        if boundary(x as f64 + 0.5, y as f64 + 0.5) > 1.25 {
            return fill;
        }
        let mut hits = 0;
        for sy in 0..16 {
            for sx in 0..16 {
                if boundary(x as f64 + (sx as f64 + 0.5) / 16., y as f64 + (sy as f64 + 0.5) / 16.)
                    <= 0.5
                {
                    hits += 1;
                }
            }
        }
        fill.map(|v| ((v as u32 * (256 - hits) + 128) / 256) as u8)
    })
}

#[test]
fn aa_blends_never_take_a_slot() {
    assert_same_set(&palette(&edge(0, 0), 12), &[RED, BLUE]);
}

#[test]
fn a_solid_patch_is_a_fill() {
    assert_same_set(&palette(&edge(16, 16), 12), &[RED, BLUE, PURPLE]);
}

#[test]
fn a_stroke_needs_an_interior_or_one_percent_ink() {
    // 1–2 px strokes have no interior and ink < 1% of the image; 3 px have both
    // an interior row and the votes.
    assert_same_set(&palette(&edge(128, 1), 12), &[RED, BLUE]);
    assert_same_set(&palette(&edge(128, 2), 12), &[RED, BLUE]);
    assert_same_set(&palette(&edge(128, 3), 12), &[RED, BLUE, PURPLE]);
}

#[test]
fn a_repeated_blend_stays_out() {
    // The purple seam between every red and blue stripe is 5% of the image,
    // well over any area gate; only geometry (no interior, not a mark) keeps it
    // out.
    let seg = segment(320, 192, |x, _| match x % 40 {
        0 | 20 => PURPLE,
        1..20 => RED,
        _ => BLUE,
    });
    assert_same_set(&palette(&seg, 12), &[RED, BLUE]);
}

#[test]
fn pooled_ink_needs_one_percent_coverage() {
    // 9 bars ink ~0.94% of the image, 10 bars ~1.04%.
    let below = palette(&slanted_ink(9), 12);
    assert_eq!(below.len(), 1, "{below:?}");
    let above = palette(&slanted_ink(10), 12);
    assert_eq!(above.len(), 2, "{above:?}");
    let [r, g, b] = above[1];
    assert!(r < 64 && r == g && g == b, "ink should be a dark gray: {above:?}");
}

#[test]
fn fills_come_before_inks() {
    let seg = outlined_fills();
    // Six fills (five warm + white) fill a budget of six; the outline ink waits.
    let six = palette(&seg, 6);
    assert_eq!(six.len(), 6, "{six:?}");
    assert!(six.iter().all(|&[r, _, _]| r > 128), "{six:?}");
    // A seventh slot goes to the outline.
    let seven = palette(&seg, 7);
    assert!(seven.starts_with(&six), "{seven:?}");
    assert_eq!(seven.len(), 7, "{seven:?}");
    assert!(seven[6].iter().all(|&v| v < 64), "outline ink should be dark: {seven:?}");
}

#[test]
fn the_palette_never_exceeds_the_budget() {
    let seg = outlined_fills();
    for k in 1..=8 {
        let p = palette(&seg, k);
        assert!(!p.is_empty() && p.len() <= k, "K={k}: {p:?}");
    }
}

#[test]
fn an_empty_segmentation_has_no_palette() {
    assert!(AutoQuantize { max_colors: 8 }.palette(&Segmentation::new(0, 0)).is_empty());
    assert!(AutoQuantize { max_colors: 8 }.palette(&Segmentation::new(4, 4)).is_empty());
}

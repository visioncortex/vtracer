//! Transparency keying, ported from the 0.6.x `converter.rs`.
//!
//! When an image has substantial transparency, fully-transparent pixels are
//! recolored to an unused "key" color so the clustering runner can treat them
//! as a discardable background. The random key search of 0.6.x is replaced by a
//! deterministic sweep so results are reproducible and `no_std`/wasm-friendly.

use visioncortex::{Color, ColorImage};

use crate::error::Error;

/// Fraction of pixels in the sampled rows that must be transparent before the
/// whole image is keyed.
const KEYING_THRESHOLD: f32 = 0.2;

/// Whether the image carries enough transparency to warrant keying.
pub fn should_key_image(img: &ColorImage) -> bool {
    if img.width == 0 || img.height == 0 {
        return false;
    }

    let threshold = ((img.width * 2) as f32 * KEYING_THRESHOLD) as usize;
    let mut transparent = 0usize;
    let rows = [
        0,
        img.height / 4,
        img.height / 2,
        3 * img.height / 4,
        img.height - 1,
    ];
    for y in rows {
        for x in 0..img.width {
            if img.get_pixel(x, y).a == 0 {
                transparent += 1;
            }
            if transparent >= threshold {
                return true;
            }
        }
    }
    false
}

fn color_exists(img: &ColorImage, color: Color) -> bool {
    for y in 0..img.height {
        for x in 0..img.width {
            let p = img.get_pixel(x, y);
            if p.r == color.r && p.g == color.g && p.b == color.b {
                return true;
            }
        }
    }
    false
}

/// Find a color not present in the image, to be used as the key. Tries the
/// primary/secondary colors first, then does a deterministic sweep of the RGB
/// cube. Returns [`Error::NoKeyColor`] only if every probed color is used.
pub fn find_unused_color(img: &ColorImage) -> Result<Color, Error> {
    let specials = [
        Color::new(255, 0, 0),
        Color::new(0, 255, 0),
        Color::new(0, 0, 255),
        Color::new(255, 255, 0),
        Color::new(0, 255, 255),
        Color::new(255, 0, 255),
    ];
    for &c in specials.iter() {
        if !color_exists(img, c) {
            return Ok(c);
        }
    }

    // Deterministic sweep: step by a value coprime-ish with 256 to spread out.
    const STEP: u16 = 37;
    let mut r = 0u16;
    while r < 256 {
        let mut g = 0u16;
        while g < 256 {
            let mut b = 0u16;
            while b < 256 {
                let c = Color::new(r as u8, g as u8, b as u8);
                if !color_exists(img, c) {
                    return Ok(c);
                }
                b += STEP;
            }
            g += STEP;
        }
        r += STEP;
    }

    Err(Error::NoKeyColor)
}

/// Recolor every fully-transparent pixel to `key`, in place.
pub fn apply_key(img: &mut ColorImage, key: Color) {
    for y in 0..img.height {
        for x in 0..img.width {
            if img.get_pixel(x, y).a == 0 {
                img.set_pixel(x, y, &key);
            }
        }
    }
}

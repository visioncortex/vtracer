//! Watershed frontend: partition invariants, the detail dial, and small-basin
//! absorption.

use vtracer::frontend::{Frontend, WatershedFrontend};
use vtracer::ColorImage;

fn image(w: usize, h: usize, f: impl Fn(usize, usize) -> (u8, u8, u8)) -> ColorImage {
    let mut pixels = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let (r, g, b) = f(x, y);
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    ColorImage {
        pixels,
        width: w,
        height: h,
    }
}

/// The core partition invariant behind the seam-free mosaic: painting the
/// layers bottom-to-top covers every canvas pixel exactly once per region —
/// i.e. the non-background masks are pairwise disjoint, and together with the
/// full-canvas background they tile the image.
fn assert_partition(seg: &vtracer::Segmentation) {
    let (w, h) = (seg.width as usize, seg.height as usize);
    // Background layer must be first and cover the full canvas.
    let bg = &seg.layers[0].mask;
    assert_eq!((bg.width(), bg.height()), (w, h), "background is full-canvas");
    assert_eq!(bg.area(), w * h, "background mask is solid");

    // Later layers are pairwise disjoint.
    let mut covered = vec![false; w * h];
    for layer in &seg.layers[1..] {
        let m = &layer.mask;
        for y in 0..m.image.height {
            for x in 0..m.image.width {
                if !m.image.get_pixel(x, y) {
                    continue;
                }
                let gx = (m.offset.x + x as i32) as usize;
                let gy = (m.offset.y + y as i32) as usize;
                assert!(gx < w && gy < h, "mask pixel out of canvas");
                assert!(!covered[gy * w + gx], "overlapping region masks");
                covered[gy * w + gx] = true;
            }
        }
    }
}

/// A flat single-color image is one region no matter the detail level.
#[test]
fn flat_image_is_one_region() {
    let img = image(24, 16, |_, _| (90, 120, 150));
    for detail in [0u8, 128, 255] {
        let seg = WatershedFrontend {
            detail,
            min_area: 0,
        }
        .segment(&img)
        .unwrap();
        assert_eq!(seg.layers.len(), 1, "detail={detail}");
        assert_partition(&seg);
    }
}

/// Two clearly separated halves form two regions, with the boundary exactly on
/// the color edge (no watershed-line pixels — the partition is gapless).
#[test]
fn two_tone_image_is_two_regions() {
    let img = image(32, 20, |x, _| {
        if x < 16 {
            (220, 40, 40)
        } else {
            (40, 60, 220)
        }
    });
    let seg = WatershedFrontend {
        detail: 128,
        min_area: 0,
    }
    .segment(&img)
    .unwrap();
    assert_eq!(seg.layers.len(), 2);
    assert_partition(&seg);
    // The non-background region is exactly one half of the canvas.
    assert_eq!(seg.layers[1].mask.area(), 16 * 20);
}

/// Raising detail never decreases the region count (the hierarchy cut is
/// monotone in the target).
#[test]
fn detail_is_monotone() {
    // A blobby gradient image with structure at several scales.
    let img = image(64, 48, |x, y| {
        let v = ((x * 4) as f64).sin() * 40.0 + ((y * 3) as f64).cos() * 40.0;
        let base = 128i32 + v as i32;
        let r = (base + ((x / 16) as i32) * 20).clamp(0, 255) as u8;
        let g = (base + ((y / 12) as i32) * 25).clamp(0, 255) as u8;
        (r, g, 128)
    });
    let mut prev = 0usize;
    for detail in [0u8, 64, 128, 192, 255] {
        let seg = WatershedFrontend {
            detail,
            min_area: 0,
        }
        .segment(&img)
        .unwrap();
        assert!(
            seg.layers.len() >= prev,
            "detail={detail}: {} < {prev}",
            seg.layers.len()
        );
        assert_partition(&seg);
        prev = seg.layers.len();
    }
    assert!(prev > 1, "highest detail should find several regions");
}

/// Small basins are absorbed into a neighbour rather than dropped: the region
/// disappears but its pixels stay covered (the partition invariant holds).
#[test]
fn min_area_absorbs_small_basins() {
    // Background plus a 3x3 fleck and a 12x12 block, all far apart in color.
    let img = image(40, 30, |x, y| {
        if (4..7).contains(&x) && (4..7).contains(&y) {
            (10, 200, 10) // 9 px fleck
        } else if (20..32).contains(&x) && (10..22).contains(&y) {
            (200, 30, 30) // 144 px block
        } else {
            (240, 240, 240)
        }
    });
    let keep = WatershedFrontend {
        detail: 255,
        min_area: 0,
    }
    .segment(&img)
    .unwrap();
    let absorb = WatershedFrontend {
        detail: 255,
        min_area: 16, // fleck (9 px) absorbed, block (144 px) kept
    }
    .segment(&img)
    .unwrap();

    assert!(keep.layers.len() > absorb.layers.len(), "fleck absorbed");
    assert_eq!(absorb.layers.len(), 2, "background + block survive");
    assert_partition(&absorb);
}

/// Output is deterministic: two runs produce identical layer geometry.
#[test]
fn deterministic() {
    let img = image(48, 32, |x, y| {
        (((x * 7 + y * 13) % 256) as u8, ((x * 3) % 256) as u8, ((y * 5) % 256) as u8)
    });
    let front = WatershedFrontend {
        detail: 160,
        min_area: 4,
    };
    let a = front.segment(&img).unwrap();
    let b = front.segment(&img).unwrap();
    assert_eq!(a.layers.len(), b.layers.len());
    for (la, lb) in a.layers.iter().zip(&b.layers) {
        assert_eq!(la.paint, lb.paint);
        assert_eq!(la.mask.offset, lb.mask.offset);
        assert_eq!(la.mask.area(), lb.mask.area());
    }
}

//! Watershed frontend: partition invariants, the detail dial, small-basin
//! absorption, and the hierarchy stack / cached re-cut behavior.

use vtracer::frontend::{Frontend, WatershedFrontend, WatershedHierarchy};
use vtracer::{Color, ColorImage, Clustering, Config, Hierarchical, Segmentation, Session};

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

/// Flatten the stacked layers top-down (later layers win), returning one layer
/// index per pixel — the partition both compositors ultimately consume.
fn flatten(seg: &Segmentation) -> Vec<usize> {
    let (w, h) = (seg.width as usize, seg.height as usize);
    let mut labels = vec![usize::MAX; w * h];
    for (li, layer) in seg.layers.iter().enumerate() {
        let m = &layer.mask;
        for y in 0..m.image.height {
            for x in 0..m.image.width {
                if m.image.get_pixel(x, y) {
                    let gx = (m.offset.x + x as i32) as usize;
                    let gy = (m.offset.y + y as i32) as usize;
                    labels[gy * w + gx] = li;
                }
            }
        }
    }
    labels
}

/// The stacked-hierarchy invariants: the bottom layer is a solid full canvas
/// (so overdraw is seam-free), every pixel is covered, and the flattened
/// partition has exactly `regions` distinct labels.
fn assert_stack(seg: &Segmentation, regions: usize) {
    let (w, h) = (seg.width as usize, seg.height as usize);
    let bottom = &seg.layers[0].mask;
    assert_eq!((bottom.width(), bottom.height()), (w, h), "bottom layer is full-canvas");
    assert_eq!(bottom.area(), w * h, "bottom layer is solid");

    let labels = flatten(seg);
    assert!(labels.iter().all(|&l| l != usize::MAX), "every pixel covered");
    let mut distinct: Vec<usize> = labels.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), regions, "flattened region count");

    // The final regions must be the topmost layers (painted after every
    // ancestor), or the flatten would not recover the partition.
    let first_final = seg.layers.len() - regions;
    assert!(
        distinct.iter().all(|&l| l >= first_final),
        "final regions are the topmost layers"
    );
}

/// Region count of a segmentation's flattened partition.
fn regions(seg: &Segmentation) -> usize {
    let mut labels = flatten(seg);
    labels.sort_unstable();
    labels.dedup();
    labels.len()
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
        assert_stack(&seg, 1);
    }
}

/// Two clearly separated halves form two regions plus their common ancestor:
/// the stack is [root, half, half] and the flatten recovers the exact split.
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
    assert_eq!(seg.layers.len(), 3, "root + two final regions");
    assert_stack(&seg, 2);
    // Each final region is exactly one half of the canvas.
    assert_eq!(seg.layers[1].mask.area(), 16 * 20);
    assert_eq!(seg.layers[2].mask.area(), 16 * 20);
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
        let k = regions(&seg);
        assert!(k >= prev, "detail={detail}: {k} < {prev}");
        assert_stack(&seg, k);
        prev = k;
    }
    assert!(prev > 1, "highest detail should find several regions");
}

/// Small basins are absorbed into a neighbour rather than dropped: the region
/// disappears but its pixels stay covered.
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

    assert!(regions(&keep) > regions(&absorb), "fleck absorbed");
    assert_eq!(regions(&absorb), 2, "background + block survive");
    assert_stack(&absorb, 2);
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

/// A cut of a prebuilt hierarchy equals the one-shot frontend — the contract
/// behind `Session`'s cached re-cut.
#[test]
fn hierarchy_recut_matches_one_shot() {
    let img = image(48, 32, |x, y| {
        (((x * 5 + y * 3) % 200) as u8, ((x / 8) * 30) as u8, ((y / 8) * 40) as u8)
    });
    let hierarchy = WatershedHierarchy::build(&img).unwrap();
    for detail in [64u8, 128, 200] {
        let recut = hierarchy.cut(&img, detail, 16);
        let one_shot = WatershedFrontend {
            detail,
            min_area: 16,
        }
        .segment(&img)
        .unwrap();
        assert_eq!(recut.layers.len(), one_shot.layers.len(), "detail={detail}");
        for (a, b) in recut.layers.iter().zip(&one_shot.layers) {
            assert_eq!(a.paint, b.paint);
            assert_eq!(a.mask.offset, b.mask.offset);
            assert_eq!(a.mask.area(), b.mask.area());
        }
    }
}

/// End-to-end through `Session`: retuning watershed detail re-cuts the cached
/// hierarchy, and the output still equals the one-shot pipeline.
#[test]
fn session_recut_matches_one_shot() {
    let img = image(48, 32, |x, y| {
        (((x * 5 + y * 3) % 200) as u8, ((x / 8) * 30) as u8, ((y / 8) * 40) as u8)
    });
    let mut session = Session::new(img.clone());
    let base = Config {
        clustering: Clustering::Watershed,
        ..Config::default()
    };
    for detail in [128u8, 200, 64] {
        let cfg = Config {
            watershed_detail: detail,
            ..base.clone()
        };
        assert_eq!(
            session.render_svg(&cfg).unwrap(),
            cfg.build().unwrap().to_svg(&img).unwrap(),
            "detail={detail}: session re-cut must match the one-shot pipeline"
        );
    }
}

/// Watershed + cutout is native: the partition reaches the mosaic untouched,
/// so two regions within one gradient step stay separate faces (the color
/// path's `merge_similar` would have rejoined them).
#[test]
fn cutout_keeps_watershed_partition() {
    // Two halves 4 gray-levels apart: close enough that the flatten merge
    // (threshold = layer_difference = 16 >= 3*4) would union them.
    let img = image(32, 20, |x, _| {
        if x < 16 {
            (100, 100, 100)
        } else {
            (104, 104, 104)
        }
    });
    let cfg = Config {
        clustering: Clustering::Watershed,
        hierarchical: Hierarchical::Cutout,
        watershed_detail: 255,
        filter_speckle: 0,
        ..Config::default()
    };
    let doc = cfg.build().unwrap().run(&img).unwrap();
    assert_eq!(
        doc.shapes.len(),
        2,
        "watershed partition must pass to the mosaic unmerged"
    );
}

/// …but identical-color neighbours still collapse into one face: regions that
/// snap to the same palette entry and share a boundary must not keep a useless
/// edge between them. (The dark region sits between them in stack order, so
/// the layer-level `MergeAdjacent` cannot be the one doing the merging — only
/// the mosaic's same-color merge can.)
#[test]
fn cutout_merges_identical_palette_faces() {
    let img = image(32, 32, |x, y| {
        if y < 16 {
            if x < 16 {
                (200, 200, 200) // A: top-left
            } else {
                (20, 20, 20) // C: top-right
            }
        } else {
            (180, 180, 180) // B: bottom, touches A
        }
    });
    let cfg = Config {
        clustering: Clustering::Watershed,
        hierarchical: Hierarchical::Cutout,
        watershed_detail: 255,
        filter_speckle: 0,
        palette: vec![Color::new(255, 255, 255), Color::new(0, 0, 0)],
        ..Config::default()
    };
    let doc = cfg.build().unwrap().run(&img).unwrap();
    assert_eq!(
        doc.shapes.len(),
        2,
        "A and B snap to the same palette color and share a boundary — one face"
    );
}

//! Rasterize-and-diff equivalence between stacked and mosaic (cutout) modes.
//!
//! Both modes render the *same* flattened partition of the image — stacked by
//! painting layers top-down, mosaic as a gapless tessellation. So their
//! rasterizations must agree in every region interior; they may differ only
//! within a thin band along region boundaries, where the two fitting paths
//! legitimately place the edge a fraction of a pixel apart. This test asserts
//! exactly that: any pixel that differs must lie within ~1–2px of a boundary.
//!
//! `resvg` is a dev-dependency, so this never enters a wasm build.

use resvg::{tiny_skia, usvg};
use vtracer::{ColorImage, Config, FitMode, Hierarchical};

/// A few smooth colored discs on a background — curved boundaries, limited
/// boundary length, no thin (1px) features.
fn blobs(w: usize, h: usize) -> ColorImage {
    let discs = [
        (28.0f64, 30.0, 18.0, (210u8, 60, 60)),
        (64.0, 40.0, 20.0, (60, 160, 90)),
        (44.0, 68.0, 16.0, (70, 90, 200)),
    ];
    let mut pixels = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let mut col = (235u8, 230, 225); // background
            for &(cx, cy, r, c) in &discs {
                let dx = x as f64 - cx;
                let dy = y as f64 - cy;
                if dx * dx + dy * dy <= r * r {
                    col = c;
                }
            }
            pixels.extend_from_slice(&[col.0, col.1, col.2, 255]);
        }
    }
    ColorImage {
        pixels,
        width: w,
        height: h,
    }
}

fn rasterize(svg: &str, w: u32, h: u32) -> Vec<u8> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("parse svg");
    let mut pixmap = tiny_skia::Pixmap::new(w, h).expect("alloc pixmap");
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    pixmap.data().to_vec()
}

/// Max per-channel difference between two RGBA pixels at index `i`.
fn pixel_diff(a: &[u8], b: &[u8], i: usize) -> u8 {
    (0..4)
        .map(|c| a[i + c].abs_diff(b[i + c]))
        .max()
        .unwrap_or(0)
}

/// Mark pixels within Chebyshev radius `r` of a color edge in either image.
fn boundary_band(a: &[u8], b: &[u8], w: usize, h: usize, r: i32) -> Vec<bool> {
    const EDGE: u8 = 24;
    let idx = |x: usize, y: usize| (y * w + x) * 4;
    let mut edge = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = idx(x, y);
            // An edge is where either rendering changes color vs its right/down
            // neighbor.
            let mut is_edge = false;
            for img in [a, b] {
                if x + 1 < w && neighbor_diff(img, i, idx(x + 1, y)) > EDGE {
                    is_edge = true;
                }
                if y + 1 < h && neighbor_diff(img, i, idx(x, y + 1)) > EDGE {
                    is_edge = true;
                }
            }
            if is_edge {
                edge[y * w + x] = true;
            }
        }
    }
    // Dilate the edge set by r.
    let mut band = vec![false; w * h];
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let mut near = false;
            'outer: for dy in -r..=r {
                for dx in -r..=r {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h && edge[ny as usize * w + nx as usize] {
                        near = true;
                        break 'outer;
                    }
                }
            }
            band[y as usize * w + x as usize] = near;
        }
    }
    band
}

fn neighbor_diff(img: &[u8], i: usize, j: usize) -> u8 {
    (0..4).map(|c| img[i + c].abs_diff(img[j + c])).max().unwrap_or(0)
}

fn assert_equivalent(mode: FitMode) {
    let (w, h) = (96usize, 96usize);
    let img = blobs(w, h);

    let stacked = Config {
        mode,
        hierarchical: Hierarchical::Stacked,
        ..Config::default()
    }
    .build()
    .unwrap()
    .to_svg(&img)
    .unwrap();

    let cutout = Config {
        mode,
        hierarchical: Hierarchical::Cutout,
        ..Config::default()
    }
    .build()
    .unwrap()
    .to_svg(&img)
    .unwrap();

    let a = rasterize(&stacked, w as u32, h as u32);
    let b = rasterize(&cutout, w as u32, h as u32);
    assert_eq!(a.len(), b.len());

    let band = boundary_band(&a, &b, w, h, 2);

    const DIFF: u8 = 40;
    let mut interior_mismatches = 0;
    for p in 0..(w * h) {
        let i = p * 4;
        if pixel_diff(&a, &b, i) > DIFF && !band[p] {
            interior_mismatches += 1;
        }
    }

    // Every real difference must live in the boundary band; interiors match.
    assert_eq!(
        interior_mismatches, 0,
        "{mode:?}: {interior_mismatches} interior pixels differ between stacked and cutout \
         (differences must be confined to the boundary band)"
    );
}

#[test]
fn stacked_and_cutout_agree_in_interiors_spline() {
    assert_equivalent(FitMode::Spline);
}

#[test]
fn stacked_and_cutout_agree_in_interiors_polygon() {
    assert_equivalent(FitMode::Polygon);
}

#[test]
fn stacked_and_cutout_agree_in_interiors_pixel() {
    assert_equivalent(FitMode::Pixel);
}

// --- seam / show-through test -------------------------------------------------

fn rasterize_on(svg: &str, w: u32, h: u32, bg: [u8; 4]) -> Vec<u8> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("parse svg");
    let mut pixmap = tiny_skia::Pixmap::new(w, h).expect("alloc pixmap");
    pixmap.fill(tiny_skia::Color::from_rgba8(bg[0], bg[1], bg[2], 255));
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    pixmap.data().to_vec()
}

/// A full-canvas-coverage image rendered in stacked mode must be fully opaque:
/// solid layers overdraw with no gaps, so nothing shows through. Show-through
/// (backdrop-dependent pixels away from the canvas edge) means seams — which is
/// exactly the hole-punching bug this guards against.
#[test]
fn stacked_has_no_seams() {
    let (w, h) = (96usize, 96usize);
    let img = blobs(w, h); // background fills the whole canvas
    let svg = Config {
        mode: FitMode::Spline,
        hierarchical: Hierarchical::Stacked,
        ..Config::default()
    }
    .build()
    .unwrap()
    .to_svg(&img)
    .unwrap();

    let white = rasterize_on(&svg, w as u32, h as u32, [255, 255, 255, 255]);
    let black = rasterize_on(&svg, w as u32, h as u32, [0, 0, 0, 255]);

    // Count backdrop-dependent pixels, ignoring the 1px canvas border (the only
    // legitimate outer-silhouette antialiasing for a full-coverage image).
    let mut show_through = 0;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = (y * w + x) * 4;
            if (0..3).any(|c| white[i + c].abs_diff(black[i + c]) > 8) {
                show_through += 1;
            }
        }
    }
    assert_eq!(
        show_through, 0,
        "stacked mode leaked {show_through} backdrop pixels — seams/holes in solid overdraw"
    );
}

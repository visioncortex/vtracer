//! Universal tracer fidelity benchmark — original vs reconstruction, blind to
//! how the reconstruction was made. Three raw metrics, each squashed to [0,1],
//! composed by geometric mean into ONE fidelity score (0 = garbage, 1 = exact):
//!
//!   psnr   sRGB PSNR over RGB. Squash: 1 − log(1+rmse)/log(256) — anchored
//!          at the two natural scales of 8-bit imagery and nothing else:
//!          rmse = 255 (full range) → 0, rmse ≤ 1 (the quantization step)
//!          saturates to 1. Equals psnr/48.13dB for rmse ≫ 1, i.e. still
//!          linear in dB, without arbitrary anchor constants.
//!   ssim   dssim-core multiscale DSSIM d (= 1/SSIM − 1) → SSIM = 1/(1+d),
//!          already a natural 0..1.
//!   patch  the "missing patch" / systematic-bias detector: bad ⟺ RGB
//!          Euclidean diff > thresh, OPEN the bad mask (1-round 4-conn
//!          erode+dilate — a slightly blurred or compressed source shifts
//!          every edge and paints ≤2px filaments along all boundaries; those
//!          vanish, real patches survive), then cluster it (visioncortex,
//!          4-conn), S = Σ area². Patch mass fraction P = √S / (w·h) — the RMS
//!          coherent-blob size as a fraction of the image. Squash: 2^(−P/0.005),
//!          so ONE coherent blob at 0.5% image mass halves the score while the
//!          same pixel count scattered as dust barely dents it. Exactly the
//!          failure mode PSNR/SSIM average away.
//!
//! Composite: weighted geometric mean, fidelity = (psnr¹ · ssim² · patch¹)^(1/4).
//! Geometric (not arithmetic) so a single collapsed axis drags the composite
//! down — a missing eye can't hide behind good global PSNR. SSIM carries double
//! weight: it tracks visual accuracy best and is the axis most robust to a
//! mildly compressed or blurred source.

use image::{GrayImage, RgbImage};
use visioncortex::BinaryImage;

/// Patch mass fraction that halves the patch subscore.
pub const PATCH_HALF: f64 = 0.005;
/// Default RGB Euclidean distance for a pixel to count as "bad".
pub const DEFAULT_THRESH: f64 = 24.0;
/// Composite weights (geometric): fidelity = (psnr^1 · ssim^2 · patch^1)^(1/4).
pub const W_PSNR: f64 = 1.0;
pub const W_SSIM: f64 = 2.0;
pub const W_PATCH: f64 = 1.0;

#[derive(Debug, Clone, Copy)]
pub struct FidelityReport {
    // raw
    pub psnr: f64,
    pub dssim: f64,
    /// sRGB RMSE — reported for reference, carries no weight (PSNR is the
    /// same MSE on a log curve; scoring both would double-weight it)
    pub rmse: f64,
    /// bad pixels (‖Δrgb‖ > thresh), before the opening
    pub bad_px: usize,
    /// 4-conn clusters of bad pixels after the opening
    pub clusters: usize,
    /// largest cluster area (px)
    pub largest: usize,
    /// √(Σ area²) — RMS coherent-blob mass, in px
    pub patch_mass: f64,
    // subscores in [0,1]
    pub s_psnr: f64,
    pub s_ssim: f64,
    pub s_patch: f64,
    /// geometric mean of the three subscores
    pub fidelity: f64,
}

fn dssim_score(a_rgb: &[u8], b_rgb: &[u8], w: usize, h: usize) -> f64 {
    let d = dssim_core::Dssim::new();
    let to = |buf: &[u8]| {
        let px: Vec<rgb::RGB<u8>> = (0..w * h)
            .map(|i| rgb::RGB {
                r: buf[i * 3],
                g: buf[i * 3 + 1],
                b: buf[i * 3 + 2],
            })
            .collect();
        d.create_image_rgb(&px, w, h).expect("dssim image")
    };
    let (val, _) = d.compare(&to(a_rgb), &to(b_rgb));
    val.into()
}

/// Compare an original against a candidate reconstruction (same
/// dimensions). `thresh` is the RGB Euclidean bad-pixel gate (use
/// [`DEFAULT_THRESH`]). Returns the report plus the raw bad-pixel mask
/// (255 = bad).
pub fn fidelity(orig: &RgbImage, cand: &RgbImage, thresh: f64) -> (FidelityReport, GrayImage) {
    assert_eq!(orig.dimensions(), cand.dimensions(), "rasters must match in size");
    let (w, h) = (orig.width() as usize, orig.height() as usize);
    let (orig_rgb, cand_rgb) = (orig.as_raw().as_slice(), cand.as_raw().as_slice());

    // PSNR + RMSE + bad-pixel binarization in one pass
    let mut sse = 0f64;
    let mut mask = vec![0u8; w * h];
    let mut bad_px = 0usize;
    let t2 = thresh * thresh;
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut d2 = 0f64;
            for c in 0..3 {
                let e = orig_rgb[i * 3 + c] as f64 - cand_rgb[i * 3 + c] as f64;
                d2 += e * e;
            }
            sse += d2;
            if d2 > t2 {
                mask[i] = 255;
                bad_px += 1;
            }
        }
    }
    let rmse = (sse / (w * h * 3) as f64).sqrt();
    let psnr = 20.0 * (255.0 / rmse.max(1e-6)).log10();

    let dssim = dssim_score(orig_rgb, cand_rgb, w, h);

    // opening: 1-round 4-conn erode + dilate. Edge-shift filaments (≤2px wide,
    // the signature of a slightly blurred/compressed source) vanish; genuine
    // missing patches survive. The reported mask keeps the raw bad pixels.
    let at = |m: &[u8], x: i64, y: i64| {
        x >= 0
            && y >= 0
            && (x as usize) < w
            && (y as usize) < h
            && m[y as usize * w + x as usize] != 0
    };
    let mut eroded = vec![0u8; w * h];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if at(&mask, x, y)
                && at(&mask, x - 1, y)
                && at(&mask, x + 1, y)
                && at(&mask, x, y - 1)
                && at(&mask, x, y + 1)
            {
                eroded[y as usize * w + x as usize] = 255;
            }
        }
    }
    let mut bin = BinaryImage::new_w_h(w, h);
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if at(&eroded, x, y)
                || at(&eroded, x - 1, y)
                || at(&eroded, x + 1, y)
                || at(&eroded, x, y - 1)
                || at(&eroded, x, y + 1)
            {
                bin.set_pixel(x as usize, y as usize, true);
            }
        }
    }

    let sizes: Vec<usize> = bin.to_clusters(false).iter().map(|c| c.size()).collect();
    let largest = sizes.iter().copied().max().unwrap_or(0);
    let patch_mass = if sizes.is_empty() {
        0.0
    } else {
        sizes
            .iter()
            .map(|&a| (a as f64) * (a as f64))
            .sum::<f64>()
            .sqrt()
    };
    let p_frac = patch_mass / (w * h) as f64;

    let s_psnr = 1.0 - (1.0 + rmse).ln() / 256f64.ln();
    let s_ssim = 1.0 / (1.0 + dssim);
    let s_patch = (-p_frac / PATCH_HALF * std::f64::consts::LN_2).exp();
    let fidelity = (s_psnr.powf(W_PSNR) * s_ssim.powf(W_SSIM) * s_patch.powf(W_PATCH))
        .powf(1.0 / (W_PSNR + W_SSIM + W_PATCH));

    (
        FidelityReport {
            psnr,
            dssim,
            rmse,
            bad_px,
            clusters: sizes.len(),
            largest,
            patch_mass,
            s_psnr,
            s_ssim,
            s_patch,
            fidelity,
        },
        GrayImage::from_raw(w as u32, h as u32, mask).expect("mask dims"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: usize, h: usize, c: [u8; 3]) -> Vec<u8> {
        (0..w * h).flat_map(|_| c).collect()
    }

    fn img(w: usize, h: usize, px: &[u8]) -> RgbImage {
        RgbImage::from_raw(w as u32, h as u32, px.to_vec()).unwrap()
    }

    #[test]
    fn identical_is_one() {
        let a = flat(64, 64, [120, 90, 200]);
        let (r, mask) = fidelity(&img(64, 64, &a), &img(64, 64, &a), DEFAULT_THRESH);
        assert_eq!(r.bad_px, 0);
        assert!(mask.as_raw().iter().all(|&m| m == 0));
        assert!((r.fidelity - 1.0).abs() < 1e-9, "fidelity {}", r.fidelity);
    }

    #[test]
    fn coherent_patch_scores_below_scattered_dust() {
        // same 256 bad pixels: one 16×16 blob vs isolated singles on a 64×64 grid
        let clean = flat(64, 64, [200, 200, 200]);
        let mut blob = clean.clone();
        for y in 24..40 {
            for x in 24..40 {
                blob[(y * 64 + x) * 3..(y * 64 + x) * 3 + 3].fill(0);
            }
        }
        let mut dust = clean.clone();
        for k in 0..256 {
            let (x, y) = ((k % 16) * 4, (k / 16) * 4); // 4px spacing: 256 singleton clusters
            dust[(y * 64 + x) * 3..(y * 64 + x) * 3 + 3].fill(0);
        }
        let (rb, _) = fidelity(&img(64, 64, &clean), &img(64, 64, &blob), DEFAULT_THRESH);
        let (rd, _) = fidelity(&img(64, 64, &clean), &img(64, 64, &dust), DEFAULT_THRESH);
        assert_eq!(rb.bad_px, 256);
        assert_eq!(rd.bad_px, 256);
        // dust vanishes under the opening entirely; the blob survives
        assert_eq!(rb.clusters, 1);
        assert_eq!(rd.clusters, 0);
        assert!((rd.s_patch - 1.0).abs() < 1e-9);
        // identical PSNR/RMSE by construction; the patch axis must separate them
        assert!((rb.rmse - rd.rmse).abs() < 1e-9);
        assert!(
            rb.s_patch < rd.s_patch * 0.25,
            "blob {} dust {}",
            rb.s_patch,
            rd.s_patch
        );
        assert!(rb.fidelity < rd.fidelity);
    }

    #[test]
    fn edge_shift_filaments_are_tolerated() {
        // a slightly blurred/compressed source shifts edges: thin bad-px lines
        // along boundaries. A 2px-wide full-width filament (256 px) must open
        // away; the same mass as a compact blob must not.
        let clean = flat(64, 64, [200, 200, 200]);
        let mut fil = clean.clone();
        for y in 30..32 {
            for x in 0..64 {
                fil[(y * 64 + x) * 3..(y * 64 + x) * 3 + 3].fill(0);
            }
        }
        let (rf, _) = fidelity(&img(64, 64, &clean), &img(64, 64, &fil), DEFAULT_THRESH);
        assert_eq!(rf.bad_px, 128);
        assert_eq!(rf.clusters, 0);
        assert!(
            (rf.s_patch - 1.0).abs() < 1e-9,
            "filament must not count as a patch"
        );
    }

    #[test]
    fn worse_is_lower() {
        let a = flat(32, 32, [100, 100, 100]);
        let mild: Vec<u8> = a.iter().map(|&v| v + 4).collect();
        let harsh: Vec<u8> = a.iter().map(|&v| v + 60).collect();
        let (rm, _) = fidelity(&img(32, 32, &a), &img(32, 32, &mild), DEFAULT_THRESH);
        let (rh, _) = fidelity(&img(32, 32, &a), &img(32, 32, &harsh), DEFAULT_THRESH);
        assert!(rm.fidelity > rh.fidelity);
        assert!(rh.fidelity < 0.4, "harsh {}", rh.fidelity);
    }
}

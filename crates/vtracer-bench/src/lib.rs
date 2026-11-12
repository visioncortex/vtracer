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
//!   patch  the "missing patch" / systematic-bias detector, on a GRAYSCALE
//!          miss mask: a pixel weighs 0 up to an RGB Euclidean diff of
//!          `thresh`, 1 from `thresh_high` on, linearly in between (quantised to
//!          0..=255 — all the math downstream is integer, so the mass is
//!          exact). A region that misses by a hair therefore counts by a
//!          hair, not in full. OPEN the mask (1-round 4-conn grayscale
//!          erode+dilate — a slightly blurred or compressed source shifts
//!          every edge and paints ≤2px filaments along all boundaries; those
//!          vanish, real patches survive), cluster its support (4-conn), a
//!          cluster's mass is the sum of its weights, S = Σ mass². Patch mass
//!          fraction P = √S / (w·h) — the RMS coherent-blob size as a
//!          fraction of the image. Squash: 2^(−P/0.005),
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

/// Patch mass fraction that halves the patch subscore.
pub const PATCH_HALF: f64 = 0.005;
/// Default RGB Euclidean distance up to which a pixel does not count as a
/// miss at all — the foot of the soft ramp.
pub const DEFAULT_THRESH: f64 = 24.0;
/// Default distance from which a pixel counts as a full miss — the top of
/// the soft ramp. Calibrated on 600 traces of 100 illustrations: with a
/// binary gate, outlines traced a hair off-black (15,15,14 for 2,2,2) were
/// flagged whole, joined into one network and zeroed excellent traces; with
/// the 24..64 ramp those score as they look and poor traces stay near zero.
pub const DEFAULT_THRESH_HIGH: f64 = 64.0;
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
    /// pixels with any miss weight (‖Δrgb‖ past the foot of the ramp),
    /// before the opening
    pub bad_px: usize,
    /// 4-conn clusters of the miss mask after the opening
    pub clusters: usize,
    /// largest cluster mass (full-miss px, rounded down)
    pub largest: usize,
    /// √(Σ mass²) — RMS coherent-blob mass, in full-miss px
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
/// dimensions) on the default-width soft ramp starting at `thresh` (use
/// [`DEFAULT_THRESH`]). Returns the report plus the raw miss mask
/// (grayscale: 0 = on target, 255 = a full miss), before the opening.
pub fn fidelity(orig: &RgbImage, cand: &RgbImage, thresh: f64) -> (FidelityReport, GrayImage) {
    fidelity_ramp(orig, cand, thresh, thresh + (DEFAULT_THRESH_HIGH - DEFAULT_THRESH))
}

/// [`fidelity`] with both ends of the soft ramp explicit: a pixel weighs 0
/// up to an RGB Euclidean distance of `thresh` and 1 from `thresh_high` on.
/// `thresh_high <= thresh` is the binary gate at `thresh` (the pre-alpha.7 metric).
pub fn fidelity_ramp(orig: &RgbImage, cand: &RgbImage, thresh: f64, thresh_high: f64) -> (FidelityReport, GrayImage) {
    assert_eq!(orig.dimensions(), cand.dimensions(), "rasters must match in size");
    let (w, h) = (orig.width() as usize, orig.height() as usize);
    let n = w * h;
    let (orig_rgb, cand_rgb) = (orig.as_raw().as_slice(), cand.as_raw().as_slice());

    // PSNR + RMSE + the grayscale miss mask in one pass. The weight is the
    // only float step of the patch axis (one correctly-rounded sqrt of an
    // integer), quantised to 0..=255; everything after it is integer.
    let mut sse = 0u64;
    let mut mask = vec![0u8; n];
    let mut bad_px = 0usize;
    let span = thresh_high - thresh;
    for i in 0..n {
        let mut d2 = 0u32;
        for c in 0..3 {
            let e = orig_rgb[i * 3 + c] as i32 - cand_rgb[i * 3 + c] as i32;
            d2 += (e * e) as u32;
        }
        sse += d2 as u64;
        let d = (d2 as f64).sqrt();
        let wt = if span > 0.0 {
            (((d - thresh) / span).clamp(0.0, 1.0) * 255.0).round() as u8
        } else if d > thresh {
            255
        } else {
            0
        };
        if wt != 0 {
            mask[i] = wt;
            bad_px += 1;
        }
    }
    let rmse = (sse as f64 / (n * 3) as f64).sqrt();
    let psnr = 20.0 * (255.0 / rmse.max(1e-6)).log10();

    let dssim = dssim_score(orig_rgb, cand_rgb, w, h);

    // opening: 1-round 4-conn grayscale erode (min) + dilate (max). Edge-shift
    // filaments (≤2px wide, the signature of a slightly blurred/compressed
    // source) vanish; genuine missing patches survive. Outside the image
    // counts as 0 for the erode and is ignored by the dilate. The reported
    // mask keeps the raw weights.
    let fold = |src: &[u8], erode: bool| {
        let mut out = vec![0u8; n];
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let mut v = src[i];
                let nb = [
                    (x > 0).then(|| src[i - 1]),
                    (x + 1 < w).then(|| src[i + 1]),
                    (y > 0).then(|| src[i - w]),
                    (y + 1 < h).then(|| src[i + w]),
                ];
                for q in nb {
                    v = match (q, erode) {
                        (Some(q), true) => v.min(q),
                        (Some(q), false) => v.max(q),
                        (None, true) => 0,
                        (None, false) => v,
                    };
                }
                out[i] = v;
            }
        }
        out
    };
    let opened = fold(&fold(&mask, true), false);

    // flood the support (4-conn), summing weight per cluster
    let mut seen = vec![false; n];
    let mut stack = Vec::new();
    let (mut clusters, mut largest, mut sum_sq) = (0usize, 0u64, 0u128);
    for s in 0..n {
        if seen[s] || opened[s] == 0 {
            continue;
        }
        seen[s] = true;
        stack.push(s);
        let mut mass = 0u64;
        while let Some(i) = stack.pop() {
            mass += opened[i] as u64;
            let (x, y) = (i % w, i / w);
            let mut go = |j: usize| {
                if !seen[j] && opened[j] != 0 {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                go(i - 1)
            }
            if x + 1 < w {
                go(i + 1)
            }
            if y > 0 {
                go(i - w)
            }
            if y + 1 < h {
                go(i + w)
            }
        }
        clusters += 1;
        largest = largest.max(mass);
        sum_sq += mass as u128 * mass as u128;
    }
    // back to pixel units: a full miss weighs 255
    let patch_mass = (sum_sq as f64).sqrt() / 255.0;
    let largest = (largest / 255) as usize;
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
            clusters,
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

    #[test]
    fn a_near_miss_counts_by_a_hair() {
        // one 16×16 blob, off by a little past the foot of the ramp vs off in
        // full: the binary gate scored these the same
        let clean = flat(64, 64, [200, 200, 200]);
        let blob = |v: u8| {
            let mut b = clean.clone();
            for y in 24..40 {
                for x in 24..40 {
                    b[(y * 64 + x) * 3..(y * 64 + x) * 3 + 3].fill(v);
                }
            }
            b
        };
        let score = |v: u8| fidelity(&img(64, 64, &clean), &img(64, 64, &blob(v)), DEFAULT_THRESH).0;
        let (hair, full) = (score(184), score(0)); // distances 27.7 and 346
        assert_eq!(hair.bad_px, 256);
        assert_eq!(hair.clusters, 1);
        assert!(hair.patch_mass < full.patch_mass * 0.15, "hair {} full {}", hair.patch_mass, full.patch_mass);
        // the opening shaves the square's four corners
        assert!((full.patch_mass - 252.0).abs() < 1e-9);
        assert!(score(190).s_patch == 1.0, "inside the foot of the ramp is no miss at all");
    }

    #[test]
    fn a_closed_ramp_is_the_binary_gate() {
        let clean = flat(64, 64, [200, 200, 200]);
        let mut b = clean.clone();
        for y in 24..40 {
            for x in 24..40 {
                b[(y * 64 + x) * 3..(y * 64 + x) * 3 + 3].fill(184);
            }
        }
        let (r, mask) = fidelity_ramp(&img(64, 64, &clean), &img(64, 64, &b), DEFAULT_THRESH, DEFAULT_THRESH);
        assert_eq!(r.largest, 252);
        assert!((r.patch_mass - 252.0).abs() < 1e-9);
        assert!(mask.as_raw().iter().all(|&m| m == 0 || m == 255));
    }
}

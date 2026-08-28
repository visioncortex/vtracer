//! Blind fidelity benchmark for raster-to-vector tracers.
//!
//!   vtracer-bench <original> <candidate> [--thresh N] [--mask out.png]
//!
//! Both arguments are rasters of identical dimensions — rendering a vector
//! reconstruction to pixels is the caller's responsibility. Prints the raw
//! metrics, their [0,1] subscores, the composite fidelity, and a
//! machine-readable csv line.

use vtracer_bench::{DEFAULT_THRESH, fidelity};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: vtracer-bench <original> <candidate> [--thresh N] [--mask out.png]");
        std::process::exit(2);
    }
    let mut thresh = DEFAULT_THRESH;
    let mut mask_out: Option<String> = None;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--thresh" => {
                i += 1;
                thresh = args[i].parse().expect("--thresh N");
            }
            "--mask" => {
                i += 1;
                mask_out = Some(args[i].clone());
            }
            a => {
                eprintln!("unknown flag {a}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let orig = image::open(&args[1]).expect("open original").to_rgb8();
    let (w, h) = (orig.width() as usize, orig.height() as usize);
    let img = image::open(&args[2]).expect("open candidate").to_rgb8();
    assert_eq!(
        (img.width() as usize, img.height() as usize),
        (w, h),
        "candidate raster must match original dimensions"
    );
    let cand: Vec<u8> = img.into_raw();

    let (r, mask) = fidelity(orig.as_raw(), &cand, w, h, thresh);

    if let Some(out) = mask_out {
        image::GrayImage::from_raw(w as u32, h as u32, mask)
            .unwrap()
            .save(&out)
            .expect("save mask");
    }

    println!(
        "psnr   {:>8.2} dB (rmse {:.2}) -> {:.4}",
        r.psnr, r.rmse, r.s_psnr
    );
    println!(
        "ssim   {:>8.5} (dssim {:.5}) -> {:.4}",
        r.s_ssim, r.dssim, r.s_ssim
    );
    println!(
        "patch  {:>8.1} px rms ({} bad px, {} clusters, largest {}) -> {:.4}",
        r.patch_mass, r.bad_px, r.clusters, r.largest, r.s_patch
    );
    println!("fidelity {:.4}", r.fidelity);
    println!(
        "[csv] {:.4},{:.2},{:.5},{:.2},{:.1},{:.4},{:.4},{:.4}",
        r.fidelity, r.psnr, r.dssim, r.rmse, r.patch_mass, r.s_psnr, r.s_ssim, r.s_patch
    );
}

# vtracer-bench

Blind fidelity benchmark for raster-to-vector tracers.

It compares an **original raster** with a **rendered reconstruction** and reports one number — a fidelity score in **[0, 1]** — built from three complementary axes. It is *blind* in the sense that it knows nothing about how the reconstruction was produced: any tracer, any format, any renderer. Render your vector output to pixels (same dimensions as the original), then let the benchmark judge.

```console
$ vtracer-bench original.png reconstruction.png
psnr      34.77 dB (rmse 4.66) -> 0.6875
ssim    0.99541 (dssim 0.00461) -> 0.9954
patch     157.8 px rms (14503 bad px, 74 clusters, largest 73) -> 0.9726
fidelity 0.9022
[csv] 0.9022,34.77,0.00461,4.66,157.8,0.6875,0.9954,0.9726
```

## Why another metric?

Every classic metric has a blind spot, and tracers exploit all of them:

- **PSNR** over-values invisible dust and undersells small salient regions — a tracer that drops an eye but nails the background can post a great PSNR.
- **SSIM** tracks perceived quality well, but averages globally: a small, fully-lost region barely moves it.
- Neither can tell **a thousand scattered ±1 pixels** apart from **one coherent missing patch** of the same total mass — and the missing patch is the failure that actually matters.

`vtracer-bench` scores all three axes and combines them so that no single blind spot survives:

| axis | raw metric | subscore in [0, 1] |
| --- | --- | --- |
| `psnr` | sRGB PSNR over RGB | `1 − log(1+rmse) / log(256)` |
| `ssim` | multiscale DSSIM (`dssim-core`) | `SSIM = 1 / (1 + DSSIM)` |
| `patch` | clustered-diff "missing patch" detector | `2^(−P / 0.005)` |

**fidelity = ( psnr¹ · ssim² · patch¹ ) ^ (1/4)** — a *weighted geometric mean*. Geometric, not arithmetic, so a single collapsed axis drags the composite down: a missing face region cannot hide behind good global PSNR. SSIM carries double weight because it tracks visual accuracy best and is the axis most robust to an imperfect source.

## The three axes

### psnr — parameter-free squash

The squash `1 − log(1+rmse)/log(256)` is anchored at the only two natural error scales an 8-bit image has:

- `rmse = 255` (the full range — noise indistinguishable from a random image) → **0**
- `rmse ≤ 1` (the quantization step — errors 8-bit can barely represent) → saturates to **1**

For `rmse ≫ 1` it equals `psnr / 48.13 dB`, i.e. it stays linear in decibels, but with no hand-picked anchor constants.

### ssim — perceptual structure

`dssim-core` computes multiscale structural dissimilarity `d = 1/SSIM − 1`; the subscore is simply `SSIM = 1/(1+d)`, already a natural 0..1. Differences the eye can't see score ~1 regardless of how many pixels they touch.

### patch — the missing-patch detector

This is the axis PSNR and SSIM both lack:

1. A pixel is **bad** iff its RGB Euclidean distance to the original exceeds `--thresh` (default 24 — roughly 14 per channel).
2. The bad mask is **opened** (one round of 4-connected erode + dilate). A slightly blurred or recompressed *source* shifts every edge and paints ≤2 px filaments along all boundaries; those vanish under the opening, while genuine missing patches survive. This is what makes the benchmark tolerant of mildly compressed or blurred originals.
3. The surviving mask is clustered (4-connected). With cluster areas `aᵢ`, the **patch mass** is `√(Σ aᵢ²)` — a sum of *squares*, so one coherent blob dominates any amount of scattered dust of equal total area.
4. With `P = patch mass / (w·h)`, the subscore is `2^(−P/0.005)`: a single coherent blob at 0.5 % of image mass halves the score; scattered dust barely dents it.

## Calibration

Scored on a 768×1024 flat-shaded illustration, comparing the original against distorted versions of **itself** — this is how much slack the benchmark gives an imperfect source, and what the top of the scale means:

| candidate | psnr | ssim | patch | **fidelity** |
| --- | --- | --- | --- | --- |
| the original itself | 1.000 | 1.000 | 1.000 | **1.0000** |
| JPEG quality 95 | 0.816 | 1.000 | 1.000 | **0.9502** |
| JPEG quality 75 | 0.718 | 0.999 | 0.994 | **0.9186** |
| 0.8 px Gaussian blur | 0.596 | 0.995 | 0.861 | **0.8443** |

Rule of thumb: **≥ 0.95** is visually indistinguishable, **≥ 0.90** is a faithful trace, **≤ 0.80** has visible geometry or color errors, and a score that *collapses* while PSNR/SSIM stay high means the patch axis found a coherent missing region — look at the `--mask` output.

## Usage

### CLI

```console
vtracer-bench <original> <candidate> [--thresh N] [--mask out.png]
```

- `original`, `candidate` — rasters of identical dimensions (any format `image` decodes). Rendering an SVG to pixels is deliberately out of scope: use the renderer whose output you actually ship (resvg, Chromium, librsvg, …) so the benchmark judges what users see.
- `--thresh N` — RGB Euclidean bad-pixel gate for the patch axis (default 24).
- `--mask out.png` — write the raw bad-pixel mask (before the opening) for visual inspection.

The last stdout line is machine-readable:

```
[csv] fidelity,psnr,dssim,rmse,patch_mass,s_psnr,s_ssim,s_patch
```

(RMSE is reported for reference but carries no weight — it is the same MSE that PSNR measures, only on a linear curve; scoring both would double-weight one error.)

### Library

```rust
use vtracer_bench::{fidelity, DEFAULT_THRESH};

// orig and cand are interleaved RGB8, both w×h
let (report, bad_mask) = fidelity(&orig, &cand, w, h, DEFAULT_THRESH);
println!("fidelity {:.4} (psnr {:.2} dB, dssim {:.5})",
    report.fidelity, report.psnr, report.dssim);
```

`FidelityReport` exposes every raw metric and subscore; the tuning constants (`PATCH_HALF`, `DEFAULT_THRESH`, and the `W_PSNR`/`W_SSIM`/`W_PATCH` weights) are public and documented in `lib.rs`.

The benchmark is fully deterministic: identical inputs produce byte-identical output.

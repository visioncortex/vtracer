# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/)
and this project adheres to [Semantic Versioning](http://semver.org/).

## 1.0.0-alpha.3 - 2026-08-01

### Added

* `vtracer-bench`: a blind fidelity benchmark for raster-to-vector tracers — it compares an original raster against a rendered reconstruction and reports one 0..1 score built from PSNR, SSIM, and a clustered-diff "missing patch" metric (geometric mean, so a single collapsed axis drags the score down). Blind to how the reconstruction was produced: render any tracer's output to pixels and score it. A new workspace crate, separate from the four shipped packages.

### Fixed

* Watershed no longer leaks a region along a blurred low-contrast crack as a 1-px filament (a slightly soft image could grow a hairline of one region's color running tens of px down a neighbouring boundary). The boundary snap's mixture gate now also admits pixels that blend two *neighbouring* regions — a blend band belongs to its closer flank even when the basin cut misattributed it to a distant region. On the blurred striped synthetic the circle's max boundary error drops from 31.6 px to 1.5 px; crisp images are byte-unaffected.

## 1.0.0-alpha.2 - 2026-07-27

### Added

* Watershed clustering (`--clustering watershed`): a new region-forming frontend — a hierarchical watershed on the pixel graph (Cousty et al. 2009; Najman et al. 2013), controlled by one dial, `--watershed-detail` (0..=255; each +25.5 roughly doubles the region count). Regions follow image content, with no watershed-line pixels.
  * Boundaries come out calm: antialiased pixels snap to the color-midpoint iso-line instead of meandering with the noise inside the ramp.
  * `stacked` stacks the merge tree itself (coarse ancestors below, refined regions on top), so overdraw stays seam-free.
  * `cutout` gets the partition natively; neighbouring faces closer than `max(2, (255 − detail) / 8)` merge, so faces a human cannot tell apart never survive as separate patches.
  * `WatershedHierarchy` is public, split into `build` (expensive, image-only) and `cut` (near-instant); `Session` re-cuts a cached hierarchy on detail changes, making the slider fully interactive (~25 ms vs ~40 ms on a 1400×775 photo).
* Curve simplification (`--simplify <tolerance>`, `Config::simplify`, `simplify` in Python and Node; off by default): a paper.js-style Schneider re-fit — each smooth run between corners is redrawn with the fewest cubics that stay within the tolerance (px). Roughly halves file size (sample photo at tolerance 1: 229 → 138 KB stacked, 103 → 36 KB watershed cutout). Runs on fitted geometry before composition, so cutout simplifies each shared boundary once and stays seam-free; corners and junction endpoints stay pinned.
* Binary thresholding: a tunable fixed threshold (`--threshold`) and Bradley–Roth adaptive thresholding for uneven lighting (`--adaptive`, `--adaptive-window`, `--adaptive-t`) — also on `Config`, Python, and Node.
* Cutout mode merges neighbouring faces whose colors are within one gradient step, rejoining the near-identical faces that stacked gradient layering splits a smooth area into.

### Changed

* `color_mode` is replaced by `clustering` (`color-cluster` | `bw` | `watershed`) across the CLI, Rust, Python, and Node — the field selects the region-forming algorithm, not a color space.
* The spline fine-tuning flags (`--corner-threshold`, `--segment-length`, `--splice-threshold`) are hidden from CLI help — still accepted, but without their `-c`/`-l`/`-s` short forms. The defaults serve virtually every conversion; `--simplify` supersedes them.

### Fixed

* Spline fitting no longer swings far away from the outline around thin strands (a long-standing defect, fixed via visioncortex 0.9.1): a sparse splice slice could be fitted by a single cubic that passed through every sample yet ballooned up to ~30 px sideways between them. Slices are now densified before fitting and multi-cubic fits kept in full, in both stacked mode and the mosaic fitter.

## 1.0.0-alpha.1 - 2026-07-24

Ground-up rewrite of VTracer into a **vectorization framework** with pluggable stages.

### Added

* Pluggable pipeline: swappable frontend (segmentation), color fitting (incl. custom palettes), curve-fitting backend, and an optimizer pass phase.
* **Mosaic mode**: true seam-free, gapless tessellation via shared boundary-graph tracing (pixel, polygon, and spline fitters), replacing the old "cutout" that produced seams.
* SVG optimizer: relative path syntax, shorthand commands, and coordinate-precision reduction for smaller files.
* `@visioncortex/vtracer` Node.js package (npm): wasm core with a native image reader.
* Rewritten Python bindings (`vtracer-py`) with a richer API; pyo3 bumped to 0.26 (fixes CPython 3.14 segfaults, #124).
* CLI accepts positional `input`/`output` arguments (#114).

### Changed

* Workspace restructured into `crates/vtracer` (core lib), `crates/vtracer-cli`, `crates/vtracer-py`, and `nodejs/`.
* CLI upgraded from clap 2.x to 4.x (#118).
* `filter_speckle` CLI cap raised from 16 to 128, matching the web app (#115).
* Depends on `visioncortex` 0.9.
* Python wheel CI now runs only on release tags and manual dispatch, not on every commit.

### Removed

* The pre-1.0 `cmdapp` crate and the demo webapp GUI.

## 0.6.12 - 2026-02-04

* Python Binding

## 0.6.5 - 2025-10-17

* Update `fastrand` to `2.3`

## 0.6.4 - 2024-03-29

* Update `visioncortex` version to `0.8.8`

## 0.6.3 - 2023-11-21

* New converter API https://github.com/visioncortex/vtracer/pull/59

## 0.6.1 - 2023-09-23

* Fixed "The two lines are parallel!"

### Python Binding

Thanks to the contribution of [@etjones](https://github.com/etjones), we now have an official Python binding! https://github.com/visioncortex/vtracer/pull/55

https://pypi.org/project/vtracer/0.6.10/

## 0.5.0 - 2022-10-09

* Handle transparent png images (cli) https://github.com/visioncortex/vtracer/pull/23

## 0.4.0 - 2021-07-23

* SVG path string numeric precision

## 0.3.0 - 2021-01-24

* Added cutout mode

## 0.2.0 - 2020-11-15

* Use relative & closed paths

## 0.1.1 - 2020-11-01

* SVG namespace

## 0.1.0 - 2020-10-31

* Initial release
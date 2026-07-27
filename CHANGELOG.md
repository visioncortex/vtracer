# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/)
and this project adheres to [Semantic Versioning](http://semver.org/).

## Unreleased

### Added

* Binary thresholding: a tunable fixed threshold and Bradley–Roth adaptive thresholding for uneven lighting — CLI `--threshold` / `--adaptive` (`--adaptive-window`, `--adaptive-t`), also on `Config`, Python, and Node.
* Cutout mode merges neighbouring mosaic regions whose colors are within one gradient step — the flattened tessellation no longer keeps the near-identical faces that stacked gradient layering splits a smooth area into.
* Watershed clustering (`--clustering watershed`): an alternative region-forming frontend — a hierarchical watershed by volume on the pixel graph (Cousty et al., TPAMI 2009; Najman, Cousty & Perret, ISMM 2013), cut at a single `--watershed-detail` dial (0..=255, each +25.5 roughly doubles the region count). Content-adaptive regions with no watershed-line pixels; antialiased boundary pixels snap to the color-midpoint iso-line, so edges come out as calm as the color-cluster frontend's instead of meandering with the pixel noise inside the ramp. With `cutout` the partition reaches the mosaic natively, and near-identical neighbouring faces merge within a detail-derived tolerance (`max(2, (255 − detail) / 8)`: the color-cluster default gradient step at the default detail, and never less than a just-noticeable difference — faces a human cannot tell apart never survive as separate patches); with `stacked` the merge tree itself is the stack — coarse ancestors below, refined regions on top, the same principle as color clustering — so sub-pixel gaps show ancestor colors and overdraw stays seam-free.
* `WatershedHierarchy` is public and split into `build` (expensive, depends only on the image) and `cut` (near-instant): `Session` builds it once and re-cuts on every `watershed_detail`/`filter_speckle` change, making the detail slider fully interactive (~25 ms re-cut vs ~40 ms rebuild on a 1400×775 photo).
* Curve simplification (`--simplify <tolerance>`, `Config::simplify`; off by default): a paper.js-style Schneider re-fit of the fitted splines — each smooth run between corners is re-fitted with the fewest cubics that stay within the tolerance (px), cutting anchor counts and file size (the 1400×775 sample photo: 229 → 138 KB stacked, 103 → 36 KB watershed cutout at tolerance 1). Implemented as a new pipeline stage (`CurvePass`) that runs on fitted geometry *before* composition, so mosaic mode simplifies each shared boundary exactly once and the tessellation stays seam-free; corners survive in place, junction endpoints are pinned bit-for-bit, and pixel/polygon polylines pass through untouched.

### Changed

* `color_mode` is replaced by `clustering` (`color-cluster` | `bw` | `watershed`) across the CLI (`--clustering`), Rust (`Config::clustering`, enum `Clustering`), Python, and Node — the field selects the region-forming algorithm, not a color space.

### Fixed

* Spline fitting no longer swings far away from the outline around thin strands (a long-standing defect, via visioncortex 0.9.1): a sparse splice slice — a few-pixel jog followed by a long straight leg — could be fitted by a single cubic that interpolated every sample while ballooning up to ~30 px sideways between them, visibly crossing narrow gaps. Slices are now densified before fitting and multi-cubic fits are kept in full, in both stacked mode and the mosaic's shared-boundary fitter.

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
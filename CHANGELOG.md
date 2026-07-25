# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/)
and this project adheres to [Semantic Versioning](http://semver.org/).

## Unreleased

### Added

* Progress reporting and cancellation for driving a UI from a worker thread: `Pipeline::run_with_progress` with a `CancelToken` and a per-phase callback.
* `Session`: interactive tuning that clusters an image once and re-renders per `Config` change, re-clustering only when a clustering parameter actually changes. Built on `Pipeline::segment`/`finish` (cache the segmentation, re-run just color/curve/optimize); `Config::segment_key` exposes what it compares.
* Binary thresholding: a tunable fixed threshold and Bradley–Roth adaptive thresholding for uneven lighting — CLI `--threshold` / `--adaptive` (`--adaptive-window`, `--adaptive-t`), also on `Config`, Python, and Node.

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
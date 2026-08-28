<div align="center">

  <img src="https://raw.githubusercontent.com/visioncortex/vtracer/master/docs/images/visioncortex-banner.png">
  <h1>VTracer</h1>

  <p>
    <strong>Raster to Vector Graphics Converter</strong>
  </p>

  <h3>
    <a href="https://www.visioncortex.org/vtracer/">Web App</a>
    <span> | </span>
    <a href="https://github.com/visioncortex/vtracer/releases/download/1.0.0-alpha.3/VTracer_1.0.0-alpha.3_x64-setup.exe">Windows App</a>
  </h3>

</div>

# vtracer (Python)

Python bindings for [`vtracer`](https://github.com/visioncortex/vtracer). Built
with [pyo3](https://pyo3.rs) + [maturin](https://www.maturin.rs); the core Rust
crate stays pure (no I/O), and this crate adds image decoding and a Pythonic API.

## Introduction

visioncortex VTracer is an open source software to convert raster images (like jpg & png) into vector graphics (svg). It can vectorize graphics and photographs and trace the curves to output compact vector files.

Comparing to Potrace, VTracer has an image processing pipeline which can handle colored images. VTracer skips Potrace's expensive optimal-polygon search in favor of a fast, linear pipeline that stays faithful to high-resolution images.

Comparing to Adobe Illustrator's Image Trace, VTracer's output is much more compact as we adopt a stacking strategy and avoid producing shapes with holes.

VTracer is originally designed for processing high resolution scans of historic blueprints up to gigapixels. At the same time, VTracer can also handle low resolution pixel art, simulating `image-rendering: pixelated` for retro game artworks.

Technical descriptions of the [tracing algorithm](https://www.visioncortex.org/vtracer-docs) and [clustering algorithm](https://www.visioncortex.org/impression-docs).

## Install

```sh
pip install vtracer==1.0.0a4
```

## Usage

```python
import vtracer

# one-liners
vtracer.convert_file("in.png", "out.svg")
svg = vtracer.convert_bytes(open("in.png", "rb").read())          # -> str
svg = vtracer.convert_pixels(rgba_bytes, width, height)           # raw RGBA8

# a rich, reusable configuration object
cfg = vtracer.Config(mode="polygon", filter_speckle=8)
cfg.hierarchical = "cutout"          # seam-free mosaic
cfg.palette = ["#1b1b1b", "#e0c088", "#5a7d3c"]   # snap to a fixed palette
cfg.max_colors = 8                   # or auto-quantize
cfg.optimize = 2
svg = cfg.convert_bytes(data)

# presets
vtracer.Config.poster().convert_file("photo.jpg", "poster.svg")
vtracer.Config.bw().convert_file("scan.png", "lineart.svg")
```

### `Config`

Constructor keyword arguments (all optional) — also exposed as mutable
properties, plus the presets `Config.bw()`, `Config.poster()`, `Config.photo()`:

| arg | default | notes |
|---|---|---|
| `clustering` | `"color-cluster"` | `"color-cluster"`, `"bw"`, or `"watershed"` |
| `hierarchical` | `"stacked"` | `"stacked"` or `"cutout"` (mosaic) |
| `mode` | `"spline"` | `"pixel"`, `"polygon"`, `"spline"` |
| `filter_speckle` | `4` | discard patches smaller than X px |
| `color_precision` | `6` | significant bits per channel |
| `layer_difference` | `16` | color diff between gradient layers |
| `corner_threshold` | `60` | degrees |
| `length_threshold` | `4.0` | px |
| `max_iterations` | `10` | |
| `splice_threshold` | `45` | degrees |
| `simplify` | `None` | curve simplification tolerance in px (try 1–2.5) |
| `path_precision` | `2` | output decimal places |
| `palette` | `None` | list of `#rrggbb` strings |
| `max_colors` | `None` | auto-quantize target |
| `optimize` | `1` | `0` off, `1` quantize+cleanup, `2` + shorthands |
| `binary_threshold` | `128` | bw: fixed cutoff, foreground below it |
| `adaptive` | `False` | bw: Bradley–Roth adaptive thresholding |
| `adaptive_window` | `0` | bw adaptive: window px (`0` = auto) |
| `adaptive_t` | `15.0` | bw adaptive: % below local mean |
| `watershed_detail` | `128` | watershed: hierarchy cut level (higher = more regions, uncapped) |

Each `Config` has `convert_file(input, output)`, `convert_bytes(data, format=None) -> str`,
and `convert_pixels(rgba, width, height) -> str`.

## Build from source

```sh
maturin develop            # into the active virtualenv
maturin build --release    # produce a wheel
```

# vtracer (Python)

Python bindings for the [`vtracer`](https://github.com/visioncortex/vtracer)
raster-to-vector framework. Built with [pyo3](https://pyo3.rs) +
[maturin](https://www.maturin.rs); the core Rust crate stays pure (no I/O), and
this crate adds image decoding and a Pythonic API.

## Install

```sh
pip install vtracer
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
| `color_mode` | `"color"` | `"color"` or `"bw"` |
| `hierarchical` | `"stacked"` | `"stacked"` or `"cutout"` (mosaic) |
| `mode` | `"spline"` | `"pixel"`, `"polygon"`, `"spline"` |
| `filter_speckle` | `4` | discard patches smaller than X px |
| `color_precision` | `6` | significant bits per channel |
| `layer_difference` | `16` | color diff between gradient layers |
| `corner_threshold` | `60` | degrees |
| `length_threshold` | `4.0` | px |
| `max_iterations` | `10` | |
| `splice_threshold` | `45` | degrees |
| `path_precision` | `2` | output decimal places |
| `palette` | `None` | list of `#rrggbb` strings |
| `max_colors` | `None` | auto-quantize target |
| `optimize` | `1` | `0` off, `1` quantize+simplify, `2` + shorthands |

Each `Config` has `convert_file(input, output)`, `convert_bytes(data, format=None) -> str`,
and `convert_pixels(rgba, width, height) -> str`.

## Build from source

```sh
maturin develop            # into the active virtualenv
maturin build --release    # produce a wheel
```

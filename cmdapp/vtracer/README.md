<div align="center">

  <img src="https://github.com/visioncortex/vtracer/raw/master/docs/images/visioncortex-banner.png">
  
  <h1>VTracer</h1>

  <p>
    <strong>Raster to Vector Graphics Converter built on top of visioncortex: Python Binding</strong>
  </p>

  <h3>
    <a href="//www.visioncortex.org/vtracer-docs">Article</a>
    <span> | </span>
    <a href="//www.visioncortex.org/vtracer/">Demo</a>
    <span> | </span>
    <a href="//github.com/visioncortex/vtracer/releases/latest">Download</a>
  </h3>

<sub>Built with 🦀 by <a href="//www.visioncortex.org/">The Vision Cortex Research Group</a></sub>

</div>

## Introduction

visioncortex VTracer is an open source software to convert raster images (like jpg & png) into vector graphics (svg). It can vectorize graphics and photographs and trace the curves to output compact vector files.

Comparing to [Potrace](http://potrace.sourceforge.net/) which only accept binarized inputs (Black & White pixmap), VTracer has an image processing pipeline which can handle colored high resolution scans.

Comparing to Adobe Illustrator's [Image Trace](https://helpx.adobe.com/illustrator/using/image-trace.html), VTracer's output is much more compact (less shapes) as we adopt a stacking strategy and avoid producing shapes with holes.

VTracer is originally designed for processing high resolution scans of historic blueprints up to gigapixels. At the same time, VTracer can also handle low resolution pixel art, simulating `image-rendering: pixelated` for retro game artworks.

A technical description of the algorithm is on [visioncortex.org/vtracer-docs](//www.visioncortex.org/vtracer-docs).

## Install (Python)

```shell
pip install vtracer
```

### Usage (Python)

```python
import vtracer

input_path = "/path/to/some_file.jpg"
output_path = "/path/to/some_file.vtracer.jpg"

# Minimal example: use all default values, generate a multicolor SVG
vtracer.convert_image_to_svg_py(inp, out)

# Single-color example. Good for line art, and much faster than full color:
vtracer.convert_image_to_svg_py(inp, out, colormode='binary')

# All the bells & whistles
vtracer.convert_image_to_svg_py(inp,
                                out,
                                colormode = 'color',        # ["color"] or "binary"
                                hierarchical = 'stacked',   # ["stacked"] or "cutout"
                                mode = 'spline',            # ["spline"] "polygon", or "none"
                                filter_speckle = 4,         # default: 4
                                color_precision = 6,        # default: 6
                                layer_difference = 16,      # default: 16
                                corner_threshold = 60,      # default: 60
                                length_threshold = 4.0,     # in [3.5, 10] default: 4.0
                                max_iterations = 10,        # default: 10
                                splice_threshold = 45,      # default: 45
                                path_precision = 3          # default: 8
                                )

```

## Web App

VTracer and its [core library](//github.com/visioncortex/visioncortex) is implemented in [Rust](//www.rust-lang.org/). It provides us a solid foundation to develop robust and efficient algorithms and easily bring it to interactive applications. The webapp is a perfect showcase of the capability of the Rust + wasm platform.

![screenshot](docs/images/screenshot-01.png)

![screenshot](docs/images/screenshot-02.png)

## Rust Library

The (Rust) library can be found on [crates.io/vtracer](//crates.io/crates/vtracer) and [crates.io/vtracer-webapp](//crates.io/crates/vtracer-webapp).

<div align="center">

  <img src="https://raw.githubusercontent.com/visioncortex/vtracer/master/docs/images/visioncortex-banner.png">
  <h1>VTracer</h1>

  <p>
    <strong>Raster to Vector Graphics Converter built on top of visioncortex</strong>
  </p>

  <h3>
    <a href="https://www.visioncortex.org/vtracer-docs">Article</a>
    <span> | </span>
    <a href="https://www.visioncortex.org/vtracer/">Demo</a>
    <span> | </span>
    <a href="https://github.com/visioncortex/vtracer/releases/latest">Download</a>
  </h3>

  <sub>Built with 🦀 by <a href="https://www.visioncortex.org/">The Vision Cortex Research Group</a></sub>
</div>

## Introduction

visioncortex VTracer is an open source software to convert raster images (like jpg & png) into vector graphics (svg). It can vectorize graphics and photographs and trace the curves to output compact vector files.

Comparing to [Potrace](http://potrace.sourceforge.net/) which only accept binarized inputs (Black & White pixmap), VTracer has an image processing pipeline which can handle colored high resolution scans.

Comparing to Adobe Illustrator's [Image Trace](https://helpx.adobe.com/illustrator/using/image-trace.html), VTracer's output is much more compact (less shapes) as we adopt a stacking strategy and avoid producing shapes with holes.

VTracer is originally designed for processing high resolution scans of historic blueprints up to gigapixels. At the same time, VTracer can also handle low resolution pixel art, simulating `image-rendering: pixelated` for retro game artworks.

A technical description of the algorithm is on [visioncortex.org/vtracer-docs](https://www.visioncortex.org/vtracer-docs).

## Cmd App

```sh
visioncortex VTracer 0.6.0
A cmd app to convert images into vector graphics.

USAGE:
    vtracer [OPTIONS] --input <input> --output <output>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
        --colormode <color_mode>                 True color image `color` (default) or Binary image `bw`
    -p, --color_precision <color_precision>      Number of significant bits to use in an RGB channel
    -c, --corner_threshold <corner_threshold>    Minimum momentary angle (degree) to be considered a corner
    -f, --filter_speckle <filter_speckle>        Discard patches smaller than X px in size
    -g, --gradient_step <gradient_step>          Color difference between gradient layers
        --hierarchical <hierarchical>
            Hierarchical clustering `stacked` (default) or non-stacked `cutout`. Only applies to color mode.

    -i, --input <input>                          Path to input raster image
    -m, --mode <mode>                            Curver fitting mode `pixel`, `polygon`, `spline`
    -o, --output <output>                        Path to output vector graphics
        --path_precision <path_precision>        Number of decimal places to use in path string
        --preset <preset>                        Use one of the preset configs `bw`, `poster`, `photo`
    -l, --segment_length <segment_length>
            Perform iterative subdivide smooth until all segments are shorter than this length

    -s, --splice_threshold <splice_threshold>    Minimum angle displacement (degree) to splice a spline
```

### Install

You can download pre-built binaries from [Releases](https://github.com/visioncortex/vtracer/releases).

You can also install the program from source from [crates.io/vtracer](https://crates.io/crates/vtracer):

```sh
cargo install vtracer
```

### Usage

```sh
./vtracer --input input.jpg --output output.svg
```

## Rust Library

You can install [`vtracer`](https://crates.io/crates/vtracer) as a Rust library.

```sh
cargo add vtracer
```

## Python Library

Since `0.6`, [`vtracer`](https://pypi.org/project/vtracer/) is also packaged as Python native extensions, thanks to the awesome [pyo3](https://github.com/PyO3/pyo3) project.

```sh
pip install vtracer
```

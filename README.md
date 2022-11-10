<div align="center">

  <img src="docs/images/visioncortex-banner.png">
  <h1>VTracer</h1>

  <p>
    <strong>Raster to Vector Graphics Converter built on top of visioncortex</strong>
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

## Web App

VTracer and its [core library](//github.com/visioncortex/visioncortex) is implemented in [Rust](//www.rust-lang.org/). It provides us a solid foundation to develop robust and efficient algorithms and easily bring it to interactive applications. The webapp is a perfect showcase of the capability of the Rust + wasm platform.

![screenshot](docs/images/screenshot-01.png)

![screenshot](docs/images/screenshot-02.png)

## Cmd App

```sh
visioncortex VTracer 0.4.0
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

### Usage
```
./vtracer --input input.jpg --output output.svg
```

## Library

The library can be found on [crates.io/vtracer](//crates.io/crates/vtracer) and [crates.io/vtracer-webapp](//crates.io/crates/vtracer-webapp).

## Install

Download pre-built binaries from [Releases](https://github.com/visioncortex/vtracer/releases).

or

Install from source (Rust toolchain needed):

```
cargo install vtracer
```

## In the wild

VTracer is used by the following products (feel free to add yours to the list):

<table>
  <tbody>
    <tr>
      <td><a href="https://filestar.com"><img src="docs/images/filestar-logo.svg" width="250"/></a>
      <br>Do anything to any file
      </td>
      <td><a href="https://logo.aliyun.com/logo#/name"><img src="docs/images/aliyun-logo.png" width="250"/></a>
      <br>Smart logo design
      </td>
      <td></td>
    </tr>
  </tbody>
</table>

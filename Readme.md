![logo](docs/images/visioncortex-banner.png)

# visioncortex VTracer

visioncortex VTracer is an open source software to convert raster images (like jpg & png) into vector graphics (svg). It can vectorize graphics and photographs and trace the curves to output compact vector files.

Comparing to [Potrace]() which only accept binarized inputs (Black & White pixmap), VTracer has an image processing pipeline which can handle colored high resolution scans.

Comparing to Adobe Illustrator's Live Trace, VTracer's output is much more compact (less shapes) as we adopt a stacking strategy and avoid producing shapes with holes.

A technical description of the algorithm is on [visioncortex.org/vtracer-docs](//www.visioncortex.org/vtracer-docs).

## Tech Stack

VTracer and its core library is implemented in [Rust](//www.rust-lang.org/). It provides us a solid foundation to develop robust and efficient algorithms and easily bring it to interactive applications. The supported target for now is WASM. It is interactive and fast, and is a perfect showcase of the capability of the Rust + HTML5 platform. We do plan to develop a command line program for VTracer.

## Screenshots

![screenshot](docs/images/screenshot-01.png)

![screenshot](docs/images/screenshot-02.png)
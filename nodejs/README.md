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

# vtracer (Node.js)

Raster → vector (SVG) for Node, a WebAssembly build of
[`vtracer`](https://github.com/visioncortex/vtracer). Image decoding and
vectorization both happen in wasm, so there is **no native dependency** — just
`npm install`.

## Introduction

visioncortex VTracer is an open source software to convert raster images (like jpg & png) into vector graphics (svg). It can vectorize graphics and photographs and trace the curves to output compact vector files.

Comparing to Potrace, VTracer has an image processing pipeline which can handle colored images. VTracer skips Potrace's expensive optimal-polygon search in favor of a fast, linear pipeline that stays faithful to high-resolution images.

Comparing to Adobe Illustrator's Image Trace, VTracer's output is much more compact as we adopt a stacking strategy and avoid producing shapes with holes.

VTracer is originally designed for processing high resolution scans of historic blueprints up to gigapixels. At the same time, VTracer can also handle low resolution pixel art, simulating `image-rendering: pixelated` for retro game artworks.

Technical descriptions of the [tracing algorithm](https://www.visioncortex.org/vtracer-docs) and [clustering algorithm](https://www.visioncortex.org/impression-docs).

## Install

```sh
npm install @visioncortex/vtracer
```

## Usage

```js
const fs = require('fs');
const vtracer = require('@visioncortex/vtracer');

// file in, file out
await vtracer.convertFile('in.png', 'out.svg');
await vtracer.convertFile('in.jpg', 'out.svg', { mode: 'polygon', hierarchical: 'cutout' });

// buffers
const svg = vtracer.convertBuffer(fs.readFileSync('in.png'), { preset: 'poster' });

// raw RGBA8 pixels
const svg2 = vtracer.convertPixels(rgba, width, height, { clustering: 'bw' });
```

## API

- `convertBuffer(buffer, options?) => string` — encoded image (PNG/JPEG/GIF/BMP) → SVG.
- `convertPixels(rgba, width, height, options?) => string` — raw RGBA8 → SVG.
- `convertFile(input, output, options?) => Promise<void>` — read, trace, write.
- `convertFileSync(input, output, options?) => void`.

### `Options` (all optional, camelCase)

`preset` (`"bw" | "poster" | "photo"`, applied first), `clustering`
(`"color-cluster" | "bw" | "watershed"`), `hierarchical` (`"stacked" | "cutout"`
for the seam-free mosaic), `mode` (`"pixel" | "polygon" | "spline"`),
`filterSpeckle`, `colorPrecision`, `layerDifference`, `cornerThreshold`,
`lengthThreshold`, `maxIterations`, `spliceThreshold`, `simplify` (curve
simplification tolerance in px, try 1–2.5), `pathPrecision`, `palette` (list of
`#rrggbb`), `maxColors`, `optimize` (`0 | 1 | 2`), `binaryThreshold` /
`adaptive` / `adaptiveWindow` / `adaptiveT` (binary mode), `watershedDetail`
(default 128; higher = more regions, uncapped).

## Build from source

Requires the Rust toolchain and [`wasm-pack`](https://rustwasm.github.io/wasm-pack/):

```sh
npm run build   # wasm-pack build --target nodejs --out-dir pkg
npm test
```

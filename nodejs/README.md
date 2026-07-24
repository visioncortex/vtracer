# vtracer (Node.js)

Raster → vector (SVG) for Node, a WebAssembly build of the
[`vtracer`](https://github.com/visioncortex/vtracer) framework. Image decoding
and vectorization both happen in wasm, so there is **no native dependency** —
just `npm install`.

## Install

```sh
npm install @visioncortex/vtracer
```

## Usage

```js
const vtracer = require('@visioncortex/vtracer');

// file in, file out
await vtracer.convertFile('in.png', 'out.svg');
await vtracer.convertFile('in.jpg', 'out.svg', { mode: 'polygon', hierarchical: 'cutout' });

// buffers
const svg = vtracer.convertBuffer(fs.readFileSync('in.png'), { preset: 'poster' });

// raw RGBA8 pixels
const svg2 = vtracer.convertPixels(rgba, width, height, { colorMode: 'bw' });
```

## API

- `convertBuffer(buffer, options?) => string` — encoded image (PNG/JPEG/GIF/BMP) → SVG.
- `convertPixels(rgba, width, height, options?) => string` — raw RGBA8 → SVG.
- `convertFile(input, output, options?) => Promise<void>` — read, trace, write.
- `convertFileSync(input, output, options?) => void`.

### `Options` (all optional, camelCase)

`preset` (`"bw" | "poster" | "photo"`, applied first), `colorMode`
(`"color" | "bw"`), `hierarchical` (`"stacked" | "cutout"` for the seam-free
mosaic), `mode` (`"pixel" | "polygon" | "spline"`), `filterSpeckle`,
`colorPrecision`, `layerDifference`, `cornerThreshold`, `lengthThreshold`,
`maxIterations`, `spliceThreshold`, `pathPrecision`, `palette` (list of
`#rrggbb`), `maxColors`, `optimize` (`0 | 1 | 2`).

## Build from source

Requires the Rust toolchain and [`wasm-pack`](https://rustwasm.github.io/wasm-pack/):

```sh
npm run build   # wasm-pack build --target nodejs --out-dir pkg
npm test
```

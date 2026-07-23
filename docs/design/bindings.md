# Bindings

Backend/CLI focused, with three language surfaces on top of `vtracer-core`. Everything except image file I/O compiles to `wasm32-unknown-unknown`.

## Python (PyPI)

Lives in the `vtracer` crate behind the `python-binding` feature (keeps the existing maturin / PyPI Trusted Publisher workflow intact).

- Ported functions with today's signatures: `convert_image_to_svg_py(image_path, out_path, **config)` and `convert_raw_image_to_svg(img_bytes, img_format=None, **config) -> str`.
- New kwargs: `palette: list[str]` (hex colors), `optimize: int`, and `hierarchical='cutout'` now meaning true mosaic.

## Wasm (`vtracer-wasm` crate)

wasm-bindgen bindings over `vtracer-core`, replacing the old `webapp/` (the GUI demo is dropped).

```text
convert(rgba: Uint8Array, width: u32, height: u32, config_json: string) -> string  // SVG
```

- Input is raw RGBA pixels — no image decoding in wasm (keeps the module small; decoding is the host's job).
- The `fastrand/js` feature wiring moves here.
- Built with `wasm-pack`; consumed by the Node.js package below and usable directly in browsers/bundlers.

## Node.js (npm)

New top-level `nodejs/` directory; recommended package name **`@visioncortex/vtracer`** (scoped — avoids collision/squatting on bare `vtracer`).

Design: wasm internally, native image reading.

- The `vtracer-wasm` build (`wasm-pack --target nodejs`) is **embedded in the package** — no network fetch, works offline.
- **[sharp](https://sharp.pixelplumbing.com/)** (native libvips binding with prebuilt binaries) decodes PNG/JPEG/WebP/GIF/AVIF/TIFF to raw RGBA, which is fed to the wasm converter. sharp is a regular dependency (this is a Node-focused library); the pixel-level API still works if the native install fails.

TypeScript API:

```ts
export interface Options {
  // camelCase mirror of the Rust Config:
  colorMode?: 'color' | 'binary';
  hierarchical?: 'stacked' | 'cutout';   // cutout = true mosaic
  mode?: 'pixel' | 'polygon' | 'spline';
  filterSpeckle?: number;
  colorPrecision?: number;
  gradientStep?: number;
  cornerThreshold?: number;
  segmentLength?: number;
  spliceThreshold?: number;
  pathPrecision?: number;
  palette?: string[];                    // ['#112233', ...]
  optimize?: 0 | 1 | 2;
}

/** Pure wasm — no native dependency needed. */
export function convertPixels(rgba: Uint8Array, width: number, height: number, options?: Options): string;

/** Decodes via sharp (native), then converts. Accepts a file path or an encoded image buffer. */
export function convertImage(input: string | Buffer, options?: Options): Promise<string>;
```

- Tests: vitest (or `node:test`) over the same sample images used by the Rust snapshot tests.
- Publishing: `npm publish` wired into the release workflow alongside crates.io and PyPI.

## visioncortex development flow

`visioncortex` stays a dependency. The workspace carries

```toml
[patch.crates-io]
visioncortex = { path = "../visioncortex" }
```

during development; API additions are committed directly to the local visioncortex repo and published as 0.8.x before a vtracer release, which then pins the published version.

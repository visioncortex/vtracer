# Architecture

## Workspace layout

```
Cargo.toml                 # workspace
crates/
├── vtracer-core/          # the framework. wasm-safe, no file/image I/O, no clap/pyo3
│   └── src/
│       ├── lib.rs
│       ├── ir/            # Segmentation, LabelMap, VectorDoc, geometry types
│       ├── frontend/      # trait Frontend + ColorClusterFrontend, BinaryFrontend, keying
│       ├── colorfit/      # trait ColorFitter + Identity, FixedPalette, AutoQuantize
│       ├── fitter/        # trait CurveFitter + Pixel, Polygon, Spline
│       ├── compose/       # stacked composition (per-region closed tracing)
│       ├── mosaic/        # boundary-graph extraction + shared-edge fitting (see mosaic.md)
│       ├── optimize/      # trait OptimizerPass + passes over VectorDoc
│       ├── svg/           # writer (absolute/relative, shorthands, precision)
│       └── pipeline.rs    # Pipeline driver + Config/presets
├── vtracer/               # publishable bin+lib crate, keeps the crate name.
│                          # image I/O (image crate), clap 4 CLI,
│                          # pyo3 binding behind `python-binding` feature
└── vtracer-wasm/          # wasm-bindgen bindings over vtracer-core
nodejs/                    # npm package: TS wrapper + embedded wasm build + sharp reader
```

- `webapp/` and `cmdapp/` are deleted (git history preserves them).
- `vtracer` re-exports `vtracer-core`, so library users need a single dependency.
- During development the workspace carries `[patch.crates-io] visioncortex = { path = "../visioncortex" }`; releases pin a published 0.8.x.
- `flo_curves` (already in the tree via visioncortex) becomes a direct dependency of `vtracer-core` for configurable-error Bezier fitting.

## Core IR

Value types from `visioncortex` are reused where they fit (`ColorImage`, `Color`, `PointF64`, `CompoundPath`); the pipeline IR is our own:

```rust
/// Frontend output — the general form is ordered layers (painter's algorithm).
pub struct Segmentation {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,          // bottom-to-top paint order
}

pub struct Layer {
    pub paint: Paint,                // starts as mean cluster color; ColorFitter may rewrite
    pub mask: RegionMask,            // the cluster's pixel indices
}

/// Flat partition for mosaic mode, derived by painting layers top-down.
pub struct LabelMap {
    pub width: u32,
    pub height: u32,
    pub labels: Vec<u32>,            // one label per pixel; u32::MAX = OUTSIDE (keyed/transparent)
    pub paints: Vec<Paint>,          // indexed by label
}

/// Output document IR — what the optimizer and the writer operate on.
pub struct VectorDoc { pub width: u32, pub height: u32, pub shapes: Vec<Shape> }
pub struct Shape     { pub paint: Paint, pub path: MultiPath }  // subpaths: MoveTo + (Line|Cubic)* + Close
pub enum   Paint     { Solid(Color) }                           // room for gradients later
```

Why layers, not a label map, as the frontend output: in stacked mode clusters genuinely overlap (each hierarchical cluster is painted over its parents), which a flat label map cannot represent. The flat `LabelMap` needed by mosaic mode is derived from the layers by a top-down flatten — cheap and lossless for that purpose.

## Stage traits

All object-safe; the driver composes boxed trait objects (ergonomic across CLI/py/wasm boundaries, negligible dispatch cost next to the per-pixel work).

```rust
pub trait Frontend {
    fn segment(&self, img: &ColorImage) -> Result<Segmentation, Error>;
}

pub trait ColorFitter {
    fn fit(&self, seg: &mut Segmentation);
}

pub trait CurveFitter {
    fn fit_closed(&self, polyline: &[PointF64]) -> Vec<PathCmd>;  // stacked outlines, rings
    fn fit_open(&self, polyline: &[PointF64]) -> Vec<PathCmd>;    // mosaic edges, endpoints pinned
}

pub trait OptimizerPass {
    fn run(&self, doc: &mut VectorDoc);
}

pub enum Compositing { Stacked, Mosaic }

pub struct Pipeline {
    pub frontend:      Box<dyn Frontend>,
    pub color_fitters: Vec<Box<dyn ColorFitter>>,
    pub fitter:        Box<dyn CurveFitter>,
    pub compositing:   Compositing,
    pub optimizers:    Vec<Box<dyn OptimizerPass>>,
}

impl Pipeline {
    pub fn run(&self, img: &ColorImage) -> Result<VectorDoc, Error> { /* driver */ }
}
```

Driver flow:

1. `frontend.segment(img)` → `Segmentation`
2. each `ColorFitter` rewrites layer paints (e.g. palette snapping)
3. compositing:
   - **Stacked** — trace each layer's closed outlines independently (port of today's `to_compound_path` flow) via `fitter.fit_closed`
   - **Mosaic** — flatten to `LabelMap`, merge adjacent same-paint regions, extract the boundary graph, fit each shared edge once via `fitter.fit_open`, assemble faces (see [mosaic.md](mosaic.md))
4. optimizer passes over the `VectorDoc`
5. `SvgWriter` serializes

## Built-in implementations

- **Frontends** (selected by `Config::clustering`)
  - `ColorClusterFrontend` — wraps `visioncortex::color_clusters::Runner`, including the transparency-keying logic that currently lives in `converter.rs` (find unused key color, key fully-transparent pixels, `KeyingAction`).
  - `BinaryFrontend` — threshold → `BinaryImage::to_clusters`.
  - `WatershedFrontend` — hierarchical watershed by volume on the 4-adjacency pixel graph (Cousty et al. TPAMI 2009; Najman, Cousty & Perret ISMM 2013), cut at `watershed_detail`. Split into `WatershedHierarchy::build` (expensive, image-only) and `cut` (near-instant), so `Session` re-cuts a cached hierarchy when the detail changes. Emits the merge tree as a stacked hierarchy (root first, refined regions on top — the color-cluster principle), so stacked mode stays seam-free and sub-pixel gaps show ancestor colors; in cutout the partition reaches the mosaic untouched (`merge_diff = 0`).
  - Third parties implement `Frontend` to feed external label maps or ML segmentation.
- **ColorFitters**
  - `Identity` (today's behavior: mean cluster color)
  - `FixedPalette { colors: Vec<Color> }` — snaps each layer paint to the nearest palette entry in OKLab
  - `AutoQuantize { max_colors }` — k-means/median-cut over layer paints
  - After palette snapping, a built-in merge step unions adjacent regions with identical paint (mosaic path) / merges consecutive identical-paint layers (stacked path).
- **CurveFitters**
  - `PixelFitter` — exact lattice polyline
  - `PolygonFitter` — staircase-symmetric Douglas-Peucker
  - `SplineFitter` — subdivision + corner detection + least-squares cubic fit (port of the visioncortex flow, extended to open polylines with pinned endpoints)

## Optimizer and SVG writer

Two levels: geometry passes over `VectorDoc`, then encoding choices in the writer.

- `QuantizePass { precision }` — round coordinates once, in document space. Replaces today's per-write rounding, and eliminates the per-path `translate(x,y)` transform by baking offsets into coordinates.
- `SimplifyPass` — drop zero-length and collinear-redundant segments *after* quantization.
- `SvgWriter { relative: bool, shorthands: bool, precision }` — per segment picks the shortest encoding:
  - relative (`l c s h v`) vs absolute deltas, whichever serializes shorter
  - `h`/`v` for axis-aligned lines, `s` for smooth cubic continuations
  - number formatting: trim trailing zeros, omit the space before negative numbers, leading-dot decimals
- Paint grouping: shapes sharing a fill emitted inside `<g fill="…">` when it saves bytes.

Output size is a tracked metric: the test suite asserts a byte-size budget against golden samples (see [roadmap.md](roadmap.md)).

## CLI

clap 4 derive, in the `vtracer` crate. Kept flags (mapping naturally): `-i/--input`, `-o/--output`, `--preset bw|poster|photo`, `--clustering color-cluster|bw|watershed` (formerly `--colormode`), `--filter_speckle`, `--color_precision`, `--gradient_step`, `--mode pixel|polygon|spline`, `--corner_threshold`, `--segment_length`, `--splice_threshold`, `--path_precision`.

New:

- `--hierarchical stacked|cutout` — `cutout` now runs the true mosaic pipeline
- `--palette '#112233,#445566,…'` / `--palette-file colors.txt` — fixed palette color fitting
- `--optimize 0..2` — optimizer level (0 = off, 1 = quantize+simplify, 2 = + full writer shorthands/grouping)
- mosaic extras: `--seam-stroke`, `--mosaic-strict` (see mosaic.md)

Range validation moves from `panic!` to clap `value_parser` ranges.

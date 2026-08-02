# VTracer 1.0 Design Documents

VTracer is being rearchitected from a single hardcoded pipeline into a **vectorization framework**. These documents describe the target design.

| Document | Contents |
|---|---|
| [architecture.md](architecture.md) | Workspace layout, core IR, stage traits, pipeline driver, optimizer & SVG writer, CLI |
| [mosaic.md](mosaic.md) | The seam-free cutout/mosaic mode: boundary-graph tracing and shared-edge curve fitting |
| [bindings.md](bindings.md) | Python (PyPI), wasm, and the new Node.js (npm) package |
| [roadmap.md](roadmap.md) | Milestones and verification strategy |

## Motivation

VTracer today (0.6.x) is a thin driver around the `visioncortex` crate: one pipeline (color clustering → per-cluster tracing → SVG string), a CLI, a pyo3 binding, and a web demo that duplicates the pipeline. The rewrite turns it into a framework with pluggable stages:

1. **Frontend** — any algorithm that produces clusters/segmentation from a raster image
2. **Curve fitting backend** — pluggable polyline→curve fitters (pixel, polygon, spline, future potrace-style)
3. **Color fitting** — mapping cluster colors to final paints, including custom fixed palettes
4. **Optimizer** — a pass pipeline that shrinks output (relative path syntax, shorthand commands, precision reduction)
5. **True mosaic cutout** — a perfect, gapless tessellation with shared boundary geometry, replacing today's fake cutout (which re-clusters a re-rendered image and shows seams)

The project stays backend/CLI focused, and everything except image file I/O compiles to `wasm32-unknown-unknown`.

## Decisions

- **`visioncortex` remains a dependency**, wrapped behind traits. Development uses a path/`[patch]` dependency on the local checkout; API additions are committed to visioncortex directly and published as 0.8.x releases. Verified that everything the new design needs is already public: the fitting primitives (`fit_points_with_bezier`, `find_corners`, `subdivide_keep_corners`, `reduce`, `PathSimplify::*`) and cluster pixel access via `ClustersView`.
- **In-repo rewrite, clean break.** New workspace layout, new API, version bump. Old CLI flags are kept only where they map naturally.
- **Python binding stays** (ported to the new API). The **webapp GUI is dropped**; a wasm library crate replaces it.
- **New Node.js library** published to npm, using the wasm build internally plus a native image reader (sharp).

## Pipeline at a glance

```
             ┌───────────┐   ┌──────────────┐   ┌─────────────────────────────┐
 raster ───▶ │ Frontend  │ ─▶│ ColorFitter* │ ─▶│ Compositing                 │
  image      │ (segment) │   │ (palette,    │   │  Stacked: closed outlines   │
             └───────────┘   │  quantize,   │   │  Mosaic:  boundary graph +  │
                             │  merge)      │   │           shared-edge fit   │
                             └──────────────┘   └──────────────┬──────────────┘
                                                               │  CurveFitter
                                                               ▼  (pixel/polygon/spline)
                                                ┌──────────────────────────────┐
                                     SVG  ◀──── │ VectorDoc ─ OptimizerPass* ─ │
                                                │            SvgWriter         │
                                                └──────────────────────────────┘
```

# Roadmap and Verification

## Milestones

Each milestone leaves the repo building and tested.

1. **Scaffold** — new workspace (`crates/vtracer-core`, `crates/vtracer`); IR + stage traits; port the existing stacked pipeline behind them, behavior-identical; golden-SVG snapshot tests over the sample images; CLI ported to clap 4 (range validation via `value_parser`, no more `panic!`).
2. **Writer + optimizer** — `VectorDoc` writer with relative/shorthand encoding, `QuantizePass`, `SimplifyPass`; byte-size benchmark vs the 0.6.x output; rasterize-and-diff regression (resvg) proving visual equivalence.
3. **Color fitting** — `FixedPalette` (OKLab nearest) + `AutoQuantize` + adjacent-region merge; `--palette` / `--palette-file` CLI.
4. **Mosaic** — boundary-graph module + open-polyline fitting (see [mosaic.md](mosaic.md)); `--hierarchical cutout` switched to the true mosaic; full unit/property test suite.
5. **Bindings** — pyo3 port, `vtracer-wasm`, the npm package under `nodejs/`; delete `webapp/`; CI covers crates.io + PyPI + npm releases.

## Verification strategy

- **Unit** — hand-crafted label maps for mosaic (checkerboard, T-junction, nested islands, border-touching, self-loops); fitter round-trips; writer encoding cases.
- **Snapshot** — golden SVGs for the sample images per preset/mode; asserted byte-size budget for the optimizer.
- **Property** (proptest) — mosaic invariants: every boundary edge used exactly twice; shoelace area == pixel counts; PixelFitter rasterize round-trip is byte-identical to the label map; fitted deviation ≤ 0.5 px budget; endpoints exact on lattice nodes.
- **Visual** — rasterize output with resvg; pixel-diff/SSIM against the input (thresholded) and against pre-rewrite output for stacked mode; mosaic diffs confined to a ~1-px boundary band.
- **Targets** — `cargo build --target wasm32-unknown-unknown -p vtracer-core -p vtracer-wasm`; `maturin build` with `python-binding`; `npm test` in `nodejs/`.

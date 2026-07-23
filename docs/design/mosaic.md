# Mosaic Mode — Seam-Free Cutout

Today's cutout re-renders the clustered image and re-clusters it, then traces every region independently; independently smoothed neighbors diverge, producing seams. The new mosaic mode replaces it with a topological pipeline that is seam-free **by construction**:

```
label map (Vec<u32>, W·H)
  → 1. boundary-graph extraction (nodes, shared segments, rings)   [integer, exact]
  → 2. face assembly (per-region contours as cycles of (seg, dir)) [integer, exact]
  → 3. fit each segment ONCE (pluggable pixel/polygon/spline)      [float, endpoints pinned]
  → 4. compose per-region SVG paths from shared fitted segments
```

Every boundary curve exists exactly once; the two adjacent regions reference the same fitted object, one traversed reversed. Reversal is exact for both polylines and cubic Beziers (`[p0,p1,p2,p3] → [p3,p2,p1,p0]`), so the serialized coordinates are identical text on both sides — no seams, no T-junction cracks.

**Coordinate convention**: pixel `(x,y)` occupies the unit square `(x,y)..(x+1,y+1)`; all boundary geometry lives on the lattice of pixel corners `0..=W × 0..=H` ("crack" boundaries). Stages 1–2 are pure integer arithmetic.

## 1. Boundary-graph extraction

### Definitions

- `type RegionId = u32; const OUTSIDE: RegionId = u32::MAX;` — `label(x,y)` returns `OUTSIDE` out of bounds. Treating outside as a real label removes all image-border special cases: border edges and border junctions fall out of the same rules.
- At lattice corner `c=(x,y)` the 2×2 pixel neighborhood is `NW NE / SW SE`. Four potential unit edges at `c`: N present iff `NW≠NE`, E iff `NE≠SE`, S iff `SW≠SE`, W iff `NW≠SW`. Degree = popcount ∈ {0, 2, 3, 4}.
- Quadrant/edge incidence for traversal: NE ↔ {N,E}, SE ↔ {E,S}, SW ↔ {S,W}, NW ↔ {W,N}.

### Node rule (junctions) and the checkerboard decision

**A corner is a node iff degree ≥ 3.**

- Three distinct labels in the 2×2 always gives degree ≥ 3 — "3+ regions meet here" is covered.
- Degree 4 with two labels is exactly the checkerboard `A B / B A` (diagonal contact). **Decision: it is a junction node of 4 edges, and faces are pinched there.** The traversal rule below always takes the sharpest right turn, staying within the current quadrant, never crossing diagonally. If clustering was 8-connected (visioncortex `diagonal: true`), a two-lobe region yields **two separate simple contours** sharing the node coordinate but no edges — emitted as one SVG path with two subpaths. Faces stay simple; the tessellation stays exact.
- Image corners (three quadrants OUTSIDE) are degree-2 chain points, not nodes. Points where two regions meet the border are degree 3 — nodes automatically.

Invariant used by segment tracing: at a degree-2 corner the 2×2 contains exactly two labels and both incident edges separate the same unordered pair — so the (left, right) region pair is constant along any chain of degree-2 corners.

### Data structures

```rust
pub type NodeId = u32;
pub type SegId  = u32;

#[derive(Clone, Copy)]
pub struct SegRef { pub seg: SegId, pub forward: bool }

pub struct Node {
    pub corner: PointI32,                 // lattice coords
    pub out: [Option<SegRef>; 4],         // outgoing directed segment per unit direction N,E,S,W
}

pub struct Segment {
    pub points: Vec<PointI32>,  // lattice polyline; len >= 2; ring: points[0] == points[last]
    pub start: Option<NodeId>,  // None,None for rings (no junction anywhere on the loop)
    pub end:   Option<NodeId>,  // start may == end (self-loop pinned at one node)
    pub left:  RegionId,        // region on the left traversing forward (y-down convention)
    pub right: RegionId,        // either side may be OUTSIDE
}

pub struct Contour(pub Vec<SegRef>);      // cycle; a ring is a 1-element contour
pub struct Face { pub region: RegionId, pub contours: Vec<Contour> }

pub struct BoundaryGraph {
    pub nodes: Vec<Node>,
    pub segments: Vec<Segment>,
    pub faces: Vec<Face>,
}
```

Transient: `corner_mask: Vec<u8>` of `(W+1)·(H+1)` (4-bit edge mask + node flag), a corner-index → `NodeId` map, and visited bitsets for undirected edges (horizontal `W·(H+1)`, vertical `(W+1)·H`; closed-form edge ids, no hashing).

"Left" in y-down screen space: heading E → left pixel above; heading S → left pixel to the east; heading W → below; heading N → to the west (4-entry lookup).

### Extraction passes

```
Pass A — classify corners: O((W+1)(H+1))
  for each lattice corner: compute 4-bit edge mask from the 2x2 labels
  (OUTSIDE for out-of-bounds); allocate a node id where popcount >= 3

Pass B — trace node-to-node segments:
  for each node n, for each present direction d not yet visited:
    walk unit edges, at each degree-2 corner continue via the unique other
    present edge, until reaching a node; record polyline, start/end nodes,
    left/right regions; register both directed views in the node tables

Pass C — closed rings:
  for each unvisited boundary edge (raster order): walk until returning to
  the start corner; record as a Segment with start = end = None
```

Complexity O(W·H + E); every boundary edge is walked exactly once here and once more during face assembly.

Corner cases handled: self-loop segments (a lobe outline returning to the same node — open for fitting purposes, endpoint pinned); whole-image single region (no nodes; Pass C finds the border rectangle as a ring against OUTSIDE); single-pixel regions.

### Successor rule (region kept on the left)

Given an incoming directed unit edge into corner `c`, tracing region R:

```
candidates in priority order: [turn_right(d_in), straight(d_in), turn_left(d_in)]
next = first d such that edge (c,d) is present AND left_pixel(c,d) == R
```

Right-first implements the pinch at checkerboard nodes (both right and straight can have R on the left there; right-first stays in the current quadrant, keeping contours simple). At 3/4-label junctions exactly one candidate qualifies. A u-turn is never needed.

## 2. Face assembly

Lift the successor rule to whole segments (two directed views per segment, 2-bit usage set):

```
for each directed segment s with region R on its left, not yet used:
    follow successor at each end node until returning to s → one Contour of R
for each ring r:
    left(r) gets [forward], right(r) gets [reversed]   (skip OUTSIDE sides)
```

**Winding falls out automatically**: interior-always-on-left gives outer contours one orientation and hole contours the opposite. Therefore each region is emitted as a single `<path fill-rule="nonzero">` whose `d` concatenates all its contours as subpaths — **no containment/nesting computation is needed**. `nonzero` (rather than `evenodd`) is robust to contours touching at pinch points.

Debug invariants: every directed segment used exactly once; per-region i64 shoelace area (holes negative) equals the region's pixel count; the global sum equals W·H minus OUTSIDE pixels.

## 3. Fitting — once per segment, endpoints pinned

```rust
pub enum FittedGeom {
    Polyline(Vec<PointF64>),        // pixel / polygon backends
    Beziers(Vec<[PointF64; 4]>),    // spline backend; consecutive curves share endpoints
}

pub trait SegmentFitter {
    fn fit_open(&self, seg: &Segment) -> FittedSegment;  // endpoints pinned to lattice nodes
    fn fit_ring(&self, seg: &Segment) -> FittedSegment;  // closed loop, no pinned point
}
```

Fitted results are cached in a `Vec<FittedSegment>` indexed by `SegId`; both adjacent faces reference the cache. Reversal happens at composition time and is exact, so shared geometry is bitwise identical — identical f64 values round identically under `path_precision`, and the emitted coordinate text matches on both sides.

### Backends

- **PixelFitter** — identity (lattice points as f64). Exact tessellation; the reference implementation for tests.
- **PolygonFitter** — symmetric open Douglas-Peucker with endpoints always kept (own ~40-line implementation). Deliberately **not** `PathSimplify::remove_staircase`: its directional outset would bias every shared boundary toward one of its two neighbors. Plain DP collapses 1-px staircases to the crack midline — centered between the two regions, which is what a mosaic wants. Self-loops split at the farthest point first.
- **SplineFitter** — open-path port of the visioncortex pipeline:
  1. DP(tau) first — staircases must be gone before corner detection, or every stair step reads as a 90° corner.
  2. Corner detection without wraparound; **both endpoints forced as corners** (junction nodes stay pinned).
  3. Open-path 4-point `subdivide_keep_corners` (no modular indexing; corner points are copied, never displaced).
  4. Open-path `find_splice_points` (inflections + accumulated-turn threshold); endpoints forced as splice points.
  5. Per slice: least-squares cubic fit. `SubdivideSmooth::fit_points_with_bezier` is already endpoint-exact (p1/p4 are taken from the input), so pinning survives fitting for free — but its internal error is hardcoded to 10.0, so vtracer-core calls `flo_curves::bezier::Curve::fit_from_points` directly with a configurable `max_error`, recursively splitting a slice at its farthest point when the budget is exceeded.
- **Rings** (islands with no junctions) are fitted once as *closed* paths using the closed-path machinery; the island uses the result forward as its outline, the enclosing region uses it reversed as a hole — same cached object, identical geometry.

### Deviation budget and overlap tolerance

Adjacent segments meet only at exact shared node coordinates — gaps are impossible. The remaining risk is a smoothed segment crossing a *different, non-adjacent* segment. Distinct boundary polylines are at least 1 px apart on the lattice, so keeping **maximum deviation < 0.5 px at every stage** (DP tau 0.5, bezier `max_error` 0.5, subdivision defaults well inside that) prevents crossings. This is not formally proven at the Bezier stage (error is sampled), so:

- default: accept the pragmatic budget — a hairline overlap between two abutting fills is visually harmless and can never produce a gap worse than the budget;
- `--mosaic-strict`: sample each fitted segment (~8 samples/curve), and fall back to the DP polyline for any segment exceeding the budget — restoring the hard guarantee at the cost of local smoothness;
- the pixel backend gives bit-exact tessellation.

## 4. Composition

Per region, one `<path fill="{color}" fill-rule="nonzero">`; the `d` string is built contour by contour, emitting each oriented segment while skipping its first point (identical to the previous segment's last point). T-junction cracks are structurally impossible: segments terminate at nodes, no curve ever spans across one, and all incident curves end at the exact integer node coordinate.

## 5. Paint-order independence and anti-aliasing

Geometric coverage is a perfect partition, so rendering is paint-order independent — the defining property of mosaic mode. Antialiasing renderers still blend a hairline along abutting edges (each path is composited independently against the backdrop); that is a renderer artifact of any abutting vector art, not a geometry defect. Optional mitigations:

1. `--seam-stroke` — stroke each path in its own fill color (`stroke-width` 0.5–1, round joins). Hides AA hairlines; reintroduces mild paint-order sensitivity (cosmetic, documented).
2. `shape-rendering="crispEdges"` output option — kills AA entirely (jaggy but seamless).
3. Stacked mode remains the AA-safe alternative (seams hidden under overdraw); mosaic gives true tessellation semantics — editable, no hidden geometry, order-free.

## Label-map source

`LabelMap::from_clusters(&ClustersView)` stamps dense region ids by iterating `clusters_output` → each cluster's pixel indices. It must **not** read `cluster_indices` directly — that maps pixels to base-level clusters, not the hierarchical output set. Unstamped (keyed/transparent) pixels become `OUTSIDE`.

## Test plan

Unit tests on hand-built const-grid label maps:

- 1×1 and full-image single region → one ring against OUTSIDE
- vertical split `A|B` → 2 border junction nodes, 3 segments, correct left/right and windings
- T-junction `A A / B C` → interior degree-3 node; three faces share the exact node coordinate
- checkerboard `A B / B A` with merged diagonal labels → degree-4 node, pinch: two simple contours touching at the point, exact coverage
- nested islands A ⊃ B ⊃ C → rings only; shared cached geometry asserted
- border-touching region, 1-px corridor, single-pixel island, self-loop segment
- reversal exactness: the two SVG coordinate substrings for a shared segment are identical strings

Property tests (proptest, random maps ≤ 12×12, ≤ 5 labels; label connectivity not required):

- every undirected boundary edge appears in exactly two directed traversals
- per-region shoelace area == pixel count; total == W·H
- **PixelFitter round-trip: scanline-rasterize the composed faces → byte-identical label map** (the strongest end-to-end guarantee; catches winding/pinch/orientation bugs)
- Polygon/Spline: sampled max deviation ≤ budget; all segment endpoints exactly on node lattice coordinates

Integration: run on the sample images; snapshot SVGs; rasterize with resvg and assert the color diff against the label map is confined to a ~1-px boundary band.

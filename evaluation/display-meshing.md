# Display geometry preparation

The preview's `mesh` timing includes tessellation, validation, normal smoothing,
CAD-edge sampling, exact volume, and tight bounds. It is broader than kernel
tessellation alone.

Stage profiling of the mounting bracket identified an exact-volume calculation
inside tessellation validation followed by a second identical query while
assembling cached geometry. The redundant query took approximately 98 ms in a
local debug build. Tessellation now returns `Tessellation { mesh, volume_mm3 }`;
scene preparation stores that validated exact volume directly. No mutable
volume cache or new invalidation rules are needed. Game tessellation retains
its separate quality settings and also returns the solid's exact volume.

Polygon extraction uses a four-point stack buffer instead of allocating a
vector for each triangle or quad. Unsupported polygons and malformed outlines
still fail. The vertex vector starts at three vertices per reported polygon,
capped by the existing 300,000-vertex budget, rather than a fixed 3,072 entries.
It can grow for quads. This reduces reallocations for larger triangle meshes
and avoids excessive initial capacity for small models.

For the 2,564-triangle bracket, tracked Rust allocations during `Body::mesh`
changed as follows (one representative warm sample per implementation):

| Allocation metric | Before | After |
| --- | ---: | ---: |
| Calls | 3,123 | 557 |
| Requested bytes | 1,242,848 | 677,792 |

Requested bytes include cumulative allocation/reallocation requests, not peak
resident memory. Kernel-internal C++ allocations are outside this counter.
Closed-mesh/orientation checks, exact-volume comparison, triangle budgets,
0.02 mm preview chord tolerance, and 15° angular tolerance remain in place.
No quality settings were relaxed.

Three separate native `--check-model` debug runs per model, with no concurrent
builds or tests, produced these medians. The baseline executable was built from
`224dc53`; both executables use the same pinned kernel and development environment.

| Model | Display before | Display after | Total before | Total after |
| --- | ---: | ---: | ---: | ---: |
| Mounting bracket | 287.7 ms | 185.7 ms | 389.3 ms | 279.1 ms |
| Four-part rack | 54.0 ms | 45.6 ms | 221.1 ms | 211.5 ms |
| Rocket engine | 119.6 ms | 97.1 ms | 144.5 ms | 121.0 ms |

Bracket display preparation takes about 35% less time and its full rebuild takes
about 28% less time in this comparison. The rack is primarily limited by CAD
construction, so it sees a smaller overall improvement. These small-sample debug
measurements are not release-build or cross-machine guarantees. Volumes and
triangle counts were unchanged for all three models. The bracket's generated
inspection contact sheet was byte-identical to the previous implementation.

All 183 workspace tests, Clippy with warnings denied, rustfmt, and the native
build passed. STEP/STL printing export, multi-part GLB export, and headless
inspection rendering succeeded; artifacts are under `output/display-meshing-*`.

Reproduce individual-stage measurements with:

```sh
nix develop -c cargo test --locked -p spice-cad profile_display_geometry_stages -- --ignored --nocapture
```

The manual profiler builds fresh bracket, four-part rack, and rocket bodies
three times each. It reports mesh-plus-validation, smoothing, edges, and tight
bounds separately, with Rust allocation counts. It is excluded from normal
test runs. A regression test bounds bracket tessellation allocation requests;
the game-mesh test distinguishes exact returned volume from approximate mesh
volume.

## Normal smoothing

Smoothing uses a pre-sized standard-library hash map instead of one tree node
allocation per few shared vertices. Each key contains the CAD face ID and all
three position bit patterns. Triangle-area contributions still accumulate in
source order; each shared normal is normalized once before being copied to its
triangle corners. Hash iteration order cannot change the sums. Separate faces,
signed-zero positions, zero/cancelled normals, and vertex metadata retain their
previous behavior.

Initial capacity estimates shared vertices as triangle count plus twice the
CAD face count, capped at the input vertex count. This covers connected face
triangulations without reserving every triangle corner; unusual disconnected
data can still grow the map. Inputs remain bounded at 300,000 vertices and
100,000 faces. No new dependencies or persistent scratch caches were added.

For the bracket, three local debug stage samples reduced smoothing median from
7.769 ms to 5.939 ms. Tracked Rust allocation calls fell from 298 to one.
Requested temporary bytes increased from 99,392 to 118,800, approximately
19 KiB more, in exchange for faster processing and fewer allocation requests.
These are stage timings; they do not establish a whole-application speedup.

Four focused regression tests cover area weighting, hard face boundaries,
metadata, cancellation, signed zero, accumulation order, idempotence, empty
inputs, and dense-mesh allocation budgets. The bracket contact sheet and rocket
GLB export were byte-identical to the previous implementation. STEP/STL export
also succeeded; artifacts are under `output/normals-*`.
All 187 workspace tests, the manual profiler, Clippy, rustfmt, and the native
build passed after this change.

## Closed-mesh validation

Validation interns the existing 0.001 mm quantized positions, then stores each
directed edge use in one 64-bit integer. Sorting those compact records replaces
the tree of coordinate-pair keys. Each edge still requires exactly two uses
with opposite directions, and the mesh must retain positive signed volume.
The 300,000-vertex bound keeps point IDs safely below the reserved direction
bit; no position precision or coordinate range was changed.

Valid meshes use only the point-index table and packed edge vector. Coordinate
lookup storage for diagnostics is allocated only after finding an invalid
edge. Error reporting still chooses the lexicographically first bad coordinate
pair and preserves its use count and orientation sum, independent of input
traversal order. Disconnected components can grow the point table within the
existing vertex budget.

Regression checks cover open edges, reversed winding, duplicate/non-manifold
surfaces, negative volume, disconnected translated components, quantized seam
tolerance, non-finite coordinates, and incomplete/empty input. A bracket
allocation check bounds valid-mesh scratch storage and allocation count.

The manual stage profiler now includes a separate **mesh validation audit**.
It repeats validation only to isolate its cost; the production mesh stage
already contains its validation pass. Do not add the audit time to total
display preparation.

Three-sample debug audit medians for the bracket compared with `0116693`:

| Validation metric | Coordinate-pair tree | Compact edge uses |
| --- | ---: | ---: |
| Time | 8.517 ms | 6.233 ms |
| Rust allocation requests | 536 | 2 |
| Requested bytes | 344,608 | 129,136 |

Valid-mesh allocation requests fall by over 99%, with approximately 63% fewer
requested scratch bytes. Complete `Body::mesh` preparation drops from roughly
557 Rust allocation requests / 677,792 bytes to 23 requests / 462,320 bytes.
Kernel-internal C++ allocations are not included. Timings varied with concurrent
host activity; the table uses comparable-load runs and does not establish an
application-wide speedup. Allocation figures remained stable across runs.

All 190 workspace tests, Clippy, rustfmt, the manual profiler, and the native
build passed. The bracket contact sheet and STL, and the rocket GLB, were
byte-identical to the previous implementation. STEP export also succeeded;
artifacts are under `output/validation-*`.

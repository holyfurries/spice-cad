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

# Interactive display refinement

Geometry-changing parameter drags use 0.25 mm / 30° display tessellation.
After pointer release and 150 ms without a parameter edit, the worker replaces
that mesh with the normal 0.02 mm / 15° mesh and completes thumbnails. The
snapshot explicitly records pending geometry refinement; the slider benchmark
only finishes once its latest revision has final geometry and thumbnails.
Completion additionally requires the viewport's prepared geometry allocation
to match the final snapshot, so receiving a refined mesh is insufficient.
Its `preview_sweep_quality final=true` line exposes that completion condition.
Final refinement consumes its idle trigger when dispatched. If it fails, the
last valid preview remains visible with the error; another parameter edit or
explicit Run can retry. Failed refinement does not create a background retry loop.

The cache keeps one active mesh per part. A final mesh also satisfies an
interactive request, so moving or rotating unchanged geometry preserves its
mesh allocation and GPU upload. Refinement reuses the canonical B-rep, exact
volume, and tight bounds without reevaluating CAD operations. It replaces the
mesh-dependent presentation and dimension-plane caches. Print export always
calls final-quality preparation, even when a drag preview is still coarse.
Game export retains its existing independent mesh generation.

Both qualities retain closed-mesh, winding, triangle-budget, and exact-volume
checks. Exact integration still uses 1e-8; its precision is unchanged. The mesh
versus exact-volume comparison uses its existing chord-dependent tolerance.
CAD edges and bounds retain their original accuracy. Arbitrary face/edge
selections clear on remeshing, as they do on geometry edits, preventing an old
triangle-derived face ID from selecting another face. Named measurements and
part selections use the existing semantic restoration path.

Run the isolated comparison without competing builds:

```sh
nix develop -c cargo test --locked -p spice-cad --all-features \
  profile_interactive_tessellation -- --ignored --nocapture
```

Five alternating-order debug samples on the mounting bracket measured
17.8–19.6 ms for interactive mesh generation plus smoothing, versus 49–55.5 ms
for final quality: 788 versus 2,564 triangles, both with 42 CAD faces. Exact
volume was primed before these samples to isolate tessellation. The rack's
flat geometry produced the same triangle counts with either quality and
showed no useful saving. Rocket parts had substantially fewer triangles and
lower mesh times at interactive quality.

These are mesh-stage measurements, not complete slider latency. First-time
exact-volume integration and CAD construction remain necessary, and final
refinement adds an extra mesh pass after dragging stops. Full interaction
measurements should use `--profile-geometry` with the same renderer, machine,
and workload before and after the change.

## Integrated verification

Release comparison on 2026-10-20 used the previous `f69fe8d` executable and
`9a59d80`, NVIDIA RTX 3070 Vulkan, and a private Xvfb display. Application
framebuffers were 1200×650. Three baseline/integrated pairs ran sequentially
without concurrent builds from this task. Other host activity was uncontrolled.
Each integrated sweep finished with `preview_sweep_quality final=true`, including
the check that the final geometry reached GPU preparation.

| Pair | Build | Accepted revisions | Control → prepare p50 ms | p95 ms | Worker p50 ms |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | Baseline | 14 | 488.741 | 757.490 | 461.574 |
| 1 | Integrated | 16 | 389.142 | 1013.246 | 364.336 |
| 2 | Baseline | 12 | 582.880 | 1238.315 | 496.755 |
| 2 | Integrated | 21 | 305.002 | 382.050 | 291.517 |
| 3 | Baseline | 24 | 256.752 | 372.681 | 237.920 |
| 3 | Integrated | 21 | 320.496 | 530.659 | 296.757 |

The third pair regressed, and tail latency remains highly variable. These runs
do not establish a reliable whole-interaction speedup or desktop smoothness.
The isolated tessellation saving above remains the narrower evidence for this
change; exact integration and other CAD work still run for changed solids.
Timing stops at GPU preparation, before presentation and physical scanout.

Geometry sweeps retained 288 KiB of geometry-buffer capacity and made three
buffer allocations, versus 544 KiB and four allocations in the baseline. These
counters exclude driver staging, textures and render targets. Separate integrated
translation/rotation sweeps completed 143/152 revisions; both kept one geometry
upload and two buffer allocations throughout. Their p50 preparation latencies
were 40.070/34.650 ms. Slider source edits coalesce while the worker is busy;
dedicated checkpoint tests cover obsolete typing/build cancellation.

The combined code passed 217 workspace tests, with two manual profilers ignored,
Clippy with all targets/features and warnings denied, Rust/Luau formatting, and
the release build. Native fixtures run serially through `.cargo/config.toml`
because they share SMLib's global context; a contended parallel run had exhausted
an existing worker-test timeout. A stressed serial run also exposed a 50 ms
wall-clock parameter-parser deadline. Parameter parsing now retains the 16 MiB
input limit and uses at most 262,144 progress callbacks, approximately 26 million
parser operations with the pinned Tree-sitter version. Large-source and budget
regressions verify that bound without timing-dependent assertions.

Manufacturing-panel STEP/STL, GLB and rendered views passed, along with four-part
rack print exports and rocket GLB. The panel is 37,552.030 mm³ with 760 final
display triangles. Raw logs and artifacts are in the ignored
`output/next-priorities/` directory.

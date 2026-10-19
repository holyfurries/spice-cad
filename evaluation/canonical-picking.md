# Canonical picking and live placement measurements

## Implementation

Rendering and picking share immutable canonical geometry through `Arc`. Face
picking transforms one world ray into each part's coordinates, rejects local
bounding-box misses, and compares hits in world space. Edge picking projects
canonical segments through the part transform and checks occlusion against all
parts. Only candidate analytic edges are transformed; no world vertex or sampled
edge arrays are retained or rebuilt for movement. Source selectors use canonical
face and edge coordinates directly.

Orientation-dependent bounds, face measurements and thumbnails still refresh on
rotation. Cached presentations no longer retain their temporary oriented meshes.
Shape changes continue to rebuild affected geometry.

## Allocation evidence

Scoped Rust allocator tests establish:

- 512 face queries plus 512 edge queries: **zero allocation calls or bytes**.
- Warm scene preparation: **7 allocation calls / 6,340 requested bytes**, unchanged
  between 372-vertex and 12,372-vertex cylinder fixtures.

These are scoped Rust requests, excluding kernel, Lua and driver allocations.
Grid-based regression tests compare picking with materialized world geometry
across rotations, both layouts, overlapping parts, near clipping and zoom levels.

A matched release heaptrack workload compared commit `6ec33e0` with this change.
Both runs used Xvfb at 1400×900 and the llvmpipe Vulkan renderer. Six held slider
sweeps produced 278 accepted changes, followed by one Undo and Assembly/Print
switches. Including initial construction and Undo, each run built 280 models,
or 560 scenes.

| Allocation calls attributed to `spice_cad::build_scene` | Before | After |
| --- | ---: | ---: |
| Total | 33,040 | 23,520 |
| Per scene | 59 | 42 |

Scene allocation calls fell **28.8%**. This does not establish a whole-process
allocation reduction: UI frame counts and unrelated allocations varied. Heaptrack's
filtered backtrace reports supplied these counts; its global histogram is not a
function-filtered byte measurement.

## Sustained drag latency

A separate release run used NVIDIA RTX 3070 Vulkan, without heaptrack. Six held
slider sweeps produced 234 renderer-accepted revisions:

| Control change to GPU prepare | Milliseconds |
| --- | ---: |
| Median | 30.142 |
| p95 | 31.443 |
| p99 | 31.850 |
| Maximum | 32.285 |

Timing starts when the parameter control handles a change and ends after the
renderer prepares that revision. It includes coalescing, worker execution and UI
scheduling; it excludes prior OS event dispatch, GPU completion and display
presentation. Superseded changes have no sample. `--profile` retains the latest
512 samples in a fixed array, calculates nearest-rank percentiles, and reports
total and retained counts on normal exit. The previous binary lacked these
timestamps, so this is a diagnostic baseline rather than a latency speedup claim.

Geometry uploads stayed at one and GPU geometry buffer allocations at two across
dragging, Undo and layout switches. Final saved values, one-step Undo and face
picking passed native checks. All 139 workspace tests, Clippy, formatting and the
release build passed.

## Reproduction

Run focused allocation checks:

```sh
nix develop -c cargo test --locked -p spice-cad picking::tests -- --nocapture
nix develop -c cargo test --locked -p spice-cad scene_preparation_allocations_are_independent_of_tessellation_density -- --nocapture
nix develop -c cargo run --release --locked -p spice-cad -- --profile
```

For the interactive run, drag rack separation continuously, release, verify the
final value, Undo, switch layouts, and close normally to emit percentiles. For
matched heaptrack runs, retain the same renderer and workload; extract scene
allocation backtraces with:

```sh
heaptrack_print -f heaptrack.zst --filter-bt-function spice_cad::build_scene -p 0 -T 0 -n 10 -s 0
```

Frame scheduling and actual input-to-presentation latency remain to be measured.

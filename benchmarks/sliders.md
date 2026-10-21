# Slider workload benchmarks

Build the native executable with the pinned environment:

```sh
nix develop
cargo build --release --locked -p spice-cad
target/release/spice-cad --profile-preview
target/release/spice-cad --profile-rotation
target/release/spice-cad --profile-geometry
```

Run these serially on the same display, renderer and framebuffer size. Each run
edits an in-memory document, ignores desktop input and closes after the final
revision reaches GPU preparation and deferred thumbnails finish. Model files are
unchanged. A missing final revision causes a timeout and unsuccessful exit.

Translation and rotation use the bundled four-part rack. Geometry changes the
four mounting-bracket holes between radii of 3 and 5 mm, retaining the same
filleted predecessor. This exercises intermediate operation reuse, booleans,
exact volume, meshing and geometry upload rather than just placement updates.

Each workload schedules 300 positions over approximately six seconds. The UI
samples the current control position every 20 ms when frames are available,
including while the worker is busy. Only the latest pending value is applied to
source when the worker becomes idle, matching slider draft coalescing. The input
timestamp follows that latest control sample through source editing, dispatch,
worker queueing, evaluation, result delivery and GPU preparation. Missed frame
ticks are skipped; they are not counted as sampled input.

The final `preview_sweep` line identifies the property and reports sampled,
coalesced and applied inputs, input-tagged worker results, and discarded stale
results. Thumbnail refreshes can produce another worker result for the same
revision. `slider_latency` reports unique prepared revisions and six latency
distributions, each retaining at most 512 samples without heap allocation.
Coalesced controls and stale results do not create false latency samples.

Compare accepted counts alongside p50/p95/p99/max: a quick final result after
many coalesced inputs does not demonstrate smooth intermediate feedback. These
measurements stop at GPU preparation; they exclude GPU completion, compositor
presentation and physical scanout. Avoid concurrent builds or profiling, and
record adapter identity and framebuffer size with results.

Regression tests cover pending-input replacement and timestamps, final-revision
completion, timeout behavior, geometry parameter bounds, rebuilding changed
holes, and reuse of an unchanged final revision.

## Integrated comparison

2026-10-20: release builds of `6d55cbd` (benchmark changes only) and `d8c5079`
(intermediate cache, volume memo, face features and game validation correction).
Three sequential baseline/integrated pairs ran on NVIDIA RTX 3070 Vulkan with
1200×650 application framebuffers in a private 1600×1000 Xvfb display. No builds
or other benchmarks from this task ran concurrently; other host activity and OS
caches were not controlled. Startup is excluded from slider timing.

| Pair | Build | Accepted revisions | Control → prepare p50 ms | p95 ms | Worker p50 ms |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | Baseline | 16 | 449.108 | 981.598 | 438.655 |
| 1 | Integrated | 21 | 291.558 | 397.949 | 289.404 |
| 2 | Baseline | 21 | 285.200 | 391.617 | 275.197 |
| 2 | Integrated | 26 | 227.615 | 387.014 | 212.542 |
| 3 | Baseline | 15 | 437.817 | 906.645 | 418.881 |
| 3 | Integrated | 26 | 238.564 | 356.346 | 229.954 |

The median of per-run medians falls from 437.817 to 238.564 ms, approximately
45%. Baseline variation is substantial; these diagnostic samples do not establish
a general desktop speedup. Every run completed its final revision with zero
stale results. Geometry-changing feedback remains a few revisions per second;
exact integration, remaining booleans and tessellation still run for changed
solids. Volume memoization helps repeated queries and export, rather than
eliminating the first integration of a changed solid.

Separate integrated translation and rotation sweeps completed 135 and 108
accepted revisions. Each retained one geometry upload and two geometry buffer
allocations for the whole sweep; only placements changed. Their control-to-prepare
medians were 42.440 and 52.209 ms on this headless display. These are not desktop
presentation measurements.

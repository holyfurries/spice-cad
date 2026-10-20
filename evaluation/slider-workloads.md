# Slider workload benchmarks

Build the native executable with the pinned environment:

```sh
nix develop -c cargo build --release --locked -p spice-cad
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

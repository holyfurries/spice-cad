# Rotation previews and deferred thumbnails

## Behavior

Parameter previews retain exact native bounds, current face measurements and
assembly constraint checks. Rotation continues to reuse canonical CPU geometry
and GPU buffers. The thumbnail rasterizer can now wait: a changed orientation,
color or shape marks its image pending during a parameter interaction. Existing
matching images remain shared, including the unchanged print layout's images.
Cards retain their size and show a viewport placeholder for pending images.

After pointer release and 150 ms without a parameter edit, the existing bounded
worker channel requests a complete preview of the current source revision. That
request fills pending images using cached geometry. A superseded result is
rejected through the same revision checks as other previews. Rendering failures
or pixel-budget exhaustion are terminal cache results, distinct from pending
images. They do not schedule repeated background work.

There is no additional worker, queue, persistent oriented mesh or idle polling
loop. Complete previews remain the default for startup, code edits and CLI
validation. Exports retain their native geometry and fabrication checks.

## Reproduce

```sh
nix develop -c cargo run --release --locked -p spice-cad -- --profile-rotation
```

The in-memory rack gains a numeric rotation property and applies Z rotation to
all four assembly parts; its print placements remain fixed. The benchmark uses
the same 300 scheduled changes, 20 ms slots and fixed 1200×650 window as the
[translation baseline](wayland-preview.md). It closes after the final revision
reaches GPU preparation and pending thumbnails finish. No model files are edited
and no desktop input is injected. `preview_sweep_thumbnails` reports cumulative
render attempts, including initial assembly and print images.

## Native Wayland measurements

Two baseline runs and two deferred-thumbnail runs used release builds, the
NVIDIA RTX 3070 Vulkan backend and the same 1200×650 Wayland window. No compilation
ran concurrently with these measurements. Each completed all 300 edits.

| Metric | Immediate thumbnails | Deferred thumbnails |
| --- | ---: | ---: |
| Control → GPU preparation median | 8.809–8.893 ms | 8.894–8.903 ms |
| Control → GPU preparation p95 | 16.978–17.083 ms | 9.214–9.221 ms |
| Worker median | 7.101–7.122 ms | 6.257–6.377 ms |
| Worker p95 | 8.472–8.557 ms | 7.464–7.541 ms |
| Result → UI p95 | 7.931–7.946 ms | 2.688–2.883 ms |

The median end-to-end delay remains approximately one refresh interval. The
smaller worker cost reduces missed-frame tails in these runs: p95 improves by
about 46%. The p99 remains approximately 17.2 ms. Two samples per configuration
are diagnostic evidence, not a statistical guarantee. Timings end at GPU
preparation and exclude GPU completion and physical presentation.

Every run retained one geometry upload, two GPU buffer allocations, 147,456
bytes of geometry capacity and 300 placement writes. Both deferred runs attempted
12 thumbnails: eight initial images across assembly/print layouts and four final
assembly images. Their completion took approximately 6.14 seconds, including the
final idle refinement, versus approximately 6.00 seconds for the baseline.

An initial baseline overlapped compilation and timed out after 36 edits; it is
excluded. The timeout cause was not isolated. Subsequent clean baseline and
candidate runs completed without model errors; the bounded benchmark deadline
remains active.

## Validation

All 145 workspace tests, Clippy with all targets/features and warnings denied,
formatting and the release build pass. Regression coverage exercises all integer
angles from 1–100 degrees with deferred images and shared geometry. Rotation
refinement preserves face centers, face spans and tight bounds, and does not
rebuild the native part. Matching images stay cached after refinement. A failing
assembly constraint still blocks a preview when thumbnails are deferred.

The native Xvfb UI exercise also passed sustained slider dragging, saving,
one-step undo back to the original model, face selection, assembly/print layout
switching, resizing and clean close. GPU geometry remained uploaded once through
those interactions. A normal launch, settled for two seconds after model upload,
recorded zero and one CPU ticks in two subsequent two-second idle samples.

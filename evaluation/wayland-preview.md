# Native Wayland preview baseline

## Reproducible workload

```sh
nix develop -c cargo run --release --locked -p spice-cad -- --profile-preview
```

This runs the bundled four-part rack in memory and exercises the existing
validated parameter-edit, preview-worker and renderer paths. It requests 300
separation values in 20 ms slots: three back-and-forth sweeps from 0–100 mm.
Missed slots coalesce to the latest value. The window requests a fixed 1200×650
size without activation; actual framebuffer dimensions appear in the profile.
No desktop input is injected. Input inside the benchmark window is ignored,
model files and recent-file history are unchanged, and the file watcher is
disabled for this in-memory document. Typed analysis remains enabled.

The process closes after the final source revision reaches GPU preparation.
Cancellation or failure to complete within the 30-second application deadline
returns a nonzero exit status. The watchdog also runs in eframe's logic callback
when the compositor withholds drawing. An external process timeout remains
appropriate when testing potentially stuck kernel or graphics calls.

The scheduled slot count differs from admitted edits and observed renderer
samples. `preview_sweep` reports applied edits, `preview_sweep_gpu` reports geometry
and placement counters, and `slider_latency` reports observed samples. Synthetic
edits time the internal control path; these results exclude physical input
dispatch, GPU completion and presentation.

## Wayland versus Xvfb

Both diagnostic runs used the same release binary, NVIDIA RTX 3070 Vulkan,
1200×650 framebuffer and sweep schedule. The Wayland window was confirmed mapped,
visible and floating through compositor metadata filtered to its process ID.

| Metric | Native Wayland | Xvfb X11 |
| --- | ---: | ---: |
| Applied and observed revisions | 300 | 276 |
| Control change → prepare median | 8.822 ms | 22.047 ms |
| Control change → prepare p95 | 9.171 ms | 23.976 ms |
| Worker build median | 2.423 ms | 2.512 ms |
| Result → UI median | 5.600 ms | 18.638 ms |
| UI → prepare median | 0.647 ms | 0.663 ms |

Each run retained one geometry upload, two GPU buffer allocations and 147,456
bytes of geometry buffer capacity. Wayland wrote 301 placement updates, including
initial construction. Placement writes can be fewer than source revisions when
two coalesced values produce the same transform.

The long delivery delay from the [earlier investigation](preview-latency.md) is
not representative of native Wayland on this desktop. Its previous 1400×900 Xvfb
workload also had a larger framebuffer; do not compare those numbers as a code
speedup. These paired runs compare display paths, not implementations. Earlier
runs measured 9.163 ms p95 on Wayland and 22.561 ms on Xvfb. Two samples per path
remain diagnostic evidence, not a controlled statistical benchmark.

The result supports retaining the current event-driven worker and renderer.
Measure native workloads before proposing a separate render thread, command
queue replacement or speculative GPU updates. Preserve geometry reuse and
sleeping idle behavior when adding further optimizations.

## Presentation limits

GPU preparation precedes surface acquisition and submission. Wayland frame
callbacks indicate compositor permission to draw another frame; they are not
presentation feedback or input-to-photon measurements. Protocol tracing and CPU
sampling should run separately from the timing baseline, and should retain only
messages belonging to the benchmark window. No presentation-thread rewrite is
justified by the headless numbers alone.

A separate filtered `WAYLAND_DEBUG=client` run matched 717 surface frame requests
to their callback IDs during the sweep. Request-to-callback delay was 7.466 ms
median, 7.774 ms p95 and 23.657 ms maximum. The selected output reports 119.998 Hz,
or approximately 8.333 ms per refresh. This is consistent with compositor-paced
drawing; callback delays are not the time a new model becomes visible on screen.

## Verification

All 144 workspace tests, Clippy with all targets/features and warnings denied,
formatting and the release build pass. Tests cover schedule coalescing, final
revision draining, timeout state, parameter bounds, stale revisions and keeping
the controls synchronized after programmatic edits. Native Wayland cancellation
closed only the owned benchmark window and returned status 1 without an unsaved
document dialog.

Three normal launches retained one geometry upload during resize tests. Early
two-second idle samples, starting 500 ms after the model marker, measured 0%,
0% and 1% of one core. A separate run settled for two seconds after that marker
and recorded zero CPU ticks in both subsequent two-second intervals. The sweep's
periodic repaint requests are confined to the explicit benchmark mode.

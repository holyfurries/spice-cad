# Preview scheduling investigation

The [native Wayland follow-up](wayland-preview.md) measures the same display
paths at matching framebuffer sizes with a self-running workload. Native Wayland
measured 9.163 ms p95 through GPU preparation; the larger headless delivery delay
below should not drive a rendering-architecture change.

## What changed

`--profile` now partitions slider control-to-GPU-prepare latency into dispatch,
worker queueing, worker execution, result delivery to the UI, and UI-to-prepare.
Every boundary follows the same source revision through the existing bounded
channels. Superseded revisions remain excluded. Six fixed 512-sample rings report
nearest-rank median, p95, p99 and maximum on normal exit. Recording and calculating
these summaries require no heap allocations.

The custom graphics configuration explicitly requests eframe's native
`SurfaceConfig::LOW_LATENCY`: automatic vsync and one queued frame, with the
library's platform exceptions. Previously, replacing the graphics configuration
implicitly selected its two-frame throughput default. This restores the native
latency policy; the measurements below do not establish a resulting speedup.
Opening another document also clears its previous slider timing tag.

## Findings

Release runs used NVIDIA RTX 3070 Vulkan, an isolated 1400×900 Xvfb window and six
held back-and-forth separation-slider sweeps, followed by Undo, face selection and
layout switches. The first run added only phase instrumentation to `b37f28a`:

| Interval | Median ms | p95 ms |
| --- | ---: | ---: |
| Control change → dispatch | 0.188 | 0.255 |
| Worker queue | 0.010 | 0.016 |
| Worker build | 2.369 | 3.272 |
| Result → UI | 27.710 | 31.306 |
| UI → GPU prepare | 0.866 | 1.295 |
| Control change → GPU prepare | 31.351 | 35.091 |

There were 229 accepted revisions. Percentiles of individual intervals do not
sum to the total percentile. Most latency occurs after the build finishes.
Worker completion already requests an immediate repaint; a regression test
checks that callback and timestamp propagation without asserting a flaky elapsed
time budget.

A one-frame vsync experiment measured 30.612 ms median / 33.094 ms p95 over 232
revisions. Immediate presentation measured 30.515 / 32.702 ms over 233 revisions.
These small differences do not establish that changing presentation modes fixes
the delay. The test surface advertised only FIFO and immediate presentation;
mailbox was unavailable. Experimental mode selection and its UI were removed.

A separate `perf` diagnostic captured 229 user-space CPU samples with no lost
samples. Main-thread stacks include Vulkan `Queue::present`, NVIDIA driver
addresses and repeated clock queries. This points to the presentation path
occupying the main thread while a preview is ready, rather than worker queue
contention. The profile includes other threads and analyzer processes; sampled
CPU percentages are not wall-time latency attribution. Traced runs are excluded
from the uninstrumented timing comparisons.

## Verification

All 142 workspace tests, Clippy with all targets/features and warnings denied,
formatting, and the release build pass. Three final-build launches recorded zero
idle CPU ticks over each two-second settled interval. Forty resize requests per
launch produced 22, 26 and 24 observed resize passes; geometry uploads stayed at
one. Resize UI CPU p95 was 0.983, 1.119 and 0.798 ms, excluding renderer and
presentation time. These checks establish sleeping idle behavior and preserved
buffer reuse, not a display latency improvement.

The final build also passed sustained dragging, exact final-value saving,
one-step slider Undo, face selection, layout switches, and typing/orbit/resize
checks. Its 227 accepted slider revisions measured 32.598 ms median / 34.477 ms
p95, consistent with the remaining delivery delay. Geometry uploads stayed at
one and buffer allocations at two. A separate native Wayland startup check on
the same adapter reached first UI at 389.763 ms and first observed model upload
at 611.624 ms. No desktop input was injected and no Wayland presentation latency
was measured.

## Reproduction and limits

```sh
nix develop -c cargo run --release --locked -p spice-cad -- --profile
```

Drag separation continuously, release, verify the final value, Undo and close
normally. Inspect each `Profile: slider_latency ... boundary=...` line. To sample
the presentation path separately:

```sh
mkdir -p output/native
nix develop -c nix-shell -p perf --run 'perf record -e cpu-clock:u -F 99 --call-graph dwarf,8192 -o output/native/preview-perf.data -- target/release/spice-cad --profile'
```

Use the same renderer, window size and workload when comparing builds. GPU prepare
precedes surface acquisition, command submission and display presentation. It is
neither GPU completion nor input-to-photon latency. Xvfb has no desktop compositor
or physical scanout, and the driver behavior here need not match a real desktop.
Capture a real-compositor presentation trace before changing rendering ownership
or adopting a separate render thread. Faster CAD execution, busy polling and a
different command queue do not address the measured delivery interval.

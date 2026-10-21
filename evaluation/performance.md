# Native performance and distribution

Historical snapshot: the legacy rack configurator, export CLI, and paired kernel
benchmark described below have been removed. Current commands and test-only
OpenCascade verification are documented in [kernel verification](../docs/kernels.md#independent-verification).

Fast startup, responsive editing and resizing, low idle cost, and simple
installation are product requirements. RAD Debugger is an architectural reference,
not a measured baseline: https://github.com/EpicGames/raddebugger.

## Current decisions

- Retain native egui and wgpu. The viewport uses persistent mesh and CAD-edge
  buffers; camera, selection, and outline controls do not rebuild geometry.
- Run Luau checking and SMLib construction on the bounded geometry worker.
  Worker completion wakes the UI; pending work does not require a 60 Hz poll.
- Watch source on a background thread with a single notification slot. Unchanged
  source does not repaint the UI. Actual reloads read the latest disk contents;
  stale notifications cannot replace a file with a previously captured snapshot.
- Cache editor syntax layout and line numbers across unchanged frames.
- Embed the starter model and create a personal copy once. Installed builds
  need neither a checkout nor developer paths.
- Preserve automatic wgpu backend selection. Backend experiments must identify
  the physical adapter and distinguish hardware from software rendering.
- Keep OpenCascade out of the normal CAD build. Shared rack dimensions and
  validation do not require a geometry kernel. The optional `rack-tools` build
  retains the older rack configurator, split-part export, and kernel comparison;
  reference dependencies are also available to tests.

## Measurement and acceptance

Use a release build on recorded hardware. Track process start to first usable UI,
first model upload, idle CPU, RSS, and interactive CPU time separately. Test
resizing during a rebuild and verify that resize events do not upload geometry.
Report initial and subsequent launches; do not describe unflushed OS caches as
a cold-start test. CPU markers do not measure presentation or input-to-photon latency.

Initial targets on the reference Linux desktop are a warm first UI within 100 ms,
the starter model within 500 ms, interactive CPU work below 4 ms at the 95th
percentile, and settled idle CPU below 1% of one logical core. These are targets,
not claims that the current implementation meets every budget. Memory needs a
separate breakdown of application allocations, mapped libraries/driver pages,
child-process peaks, and GPU allocations before setting a useful heap budget.

The repeatable Linux X11 harness owns its Xvfb server and application processes:

```sh
nix develop -c rustc --edition 2024 -O evaluation/profile-native.rs -o output/native/profile-native
nix develop -c nix-shell -p xorg-server xdotool --run \
  'output/native/profile-native target/release/spice-cad output/native/profile-new'
```

Use a new output directory. The harness records three launches, adapter identity,
RSS, settled idle CPU, and forty resize requests during a rebuild. A software
OpenGL run under Xvfb used llvmpipe while Vulkan used the RTX 3070; their startup
numbers are not a valid hardware-backend comparison. Software OpenGL results did
not change the application default. Native compositor resize and presentation
behavior still require real-desktop testing on each supported platform.

Pass two executable paths before the output directory to compare builds. The
harness alternates them for each of three samples, rejects adapter changes, and
records variant indices and startup phases. This reduces ordering drift without
claiming a statistically controlled benchmark or clearing driver/OS caches.

`--profile` also reports `before_event_loop`, `adapter_selected`,
`device_and_egui_ready`, and `viewport_ready`. These locate startup cost without
changing backend selection or required device features. One warm Vulkan trace
spent about 91 ms reaching adapter selection, 207 ms in device/egui setup,
14 ms creating the CAD pipelines, and 6 ms building the first UI pass.
The device/egui interval still combines multiple operations; it does not prove
that the driver or egui alone causes that delay. Syscall tracing is diagnostic
only and its timings must not be compared with untraced launches.

The first-pass results are in [performance-results.json](performance-results.json).
On the RTX 3070, three development-release launches with Vulkan selected reached
the first UI in 393–525 ms and the starter upload in 786–950 ms. Settled idle CPU
was zero ticks over each two-second measurement interval. The packaged build
also measured zero idle ticks and successfully loaded the starter without the
development-library environment. Its automatic-backend runs took 454–877 ms to
the first UI. These small local samples show that the startup targets remain
unmet. They do not isolate packaging overhead: backend enumeration differed.

The dependency split and latest alternating package measurements are recorded in
[kernel-split-results.json](kernel-split-results.json). Both variants used Vulkan
on the same RTX 3070, with development-library variables removed:

| Metric | With rack tools | Lean CAD |
| --- | ---: | ---: |
| Nix runtime closure | 353.3 MiB | 116.7 MiB |
| Median first UI, three samples | 383 ms | 363 ms |
| Median process RSS | 253.6 MiB | 232.2 MiB |
| Median device/egui interval | 217 ms | 219 ms |
| Median CAD pipeline interval | 16 ms | 16 ms |

The large gain is distribution size. The measured startup reduction is modest
and remains above target. Graphics setup did not improve from removing the
legacy kernel; that is the next profiling boundary to investigate. Both package
variants built successfully, the lean editor passed all sixteen window checks,
and the rack-tools GUI exported all seventeen expected files.

## Installation and next priorities

`nix run .#spice-cad` and `nix profile install .#spice-cad` provide a complete Linux
runtime including the Luau analyzer and graphics-loader libraries. This improves
installation for Nix users. It is not a cross-distribution, single-file release.

SMLib and the Luau VM link statically. The normal executable now has six direct
shared-library requirements: TBB, the C++ and GCC runtimes, libc, libm, and the
ELF loader. Graphics libraries are loaded at runtime. Typed analysis still
launches a separate analyzer. The Nix runtime closure fell from 353.3 MiB to
116.7 MiB after separating OpenCascade. This is the complete closure size;
the executable alone is approximately 33 MiB.

Use `nix run .#spice-cad-rack-tools -- --rack` for the optional configurator, or
build it with `cargo run --release --locked -p spice-cad --features rack-tools
-- --rack`. The original `cargo run --locked -p spice-rack` export CLI remains
available. Neither optional tool is required to build or install the normal CAD
application. Tests still import native STEP exports through OpenCascade to
check geometry independently.

Next, separate device creation from egui renderer initialization in the profiling
data, package or embed analysis without developer tooling on PATH, and measure
GPU initialization and application allocations on the real desktop.
Produce a portable Linux release with bundled application dependencies and
system graphics drivers, and a native Windows release when that platform is
implemented and tested. Prefer one download and launch over an absolute
fully-static-link requirement; validate either artifact on a clean machine.

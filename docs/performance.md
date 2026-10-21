# Performance

[← Spice CAD](../README.md) · Commands run from the repository root.

`spice-cad --profile` logs the first UI pass, first observed geometry upload,
and resize passes. Startup markers also separate event-loop/adapter selection,
device and egui initialization, and CAD viewport pipeline creation. These are
CPU-side markers, not GPU presentation timings. The renderer remains wgpu with
automatic backend selection; `WGPU_BACKEND=vulkan` or `WGPU_BACKEND=gl` can
select a backend for controlled comparisons.

See [performance goals and measurements](../evaluation/performance.md) for the
measurement method, installation limits, and next priorities. A Linux X11
startup, idle-CPU, memory, and resize harness is in
`evaluation/profile-native.rs`. The [cache audit](../evaluation/cache-audit.md)
records ownership, invalidation, heap profiles, and remaining repeated work.
Display preparation reuses the exact volume already calculated during mesh
validation and extracts polygons with a fixed point buffer. See
[display meshing measurements](../evaluation/display-meshing.md) for stage timings,
allocation budgets, and the manual profiler command.
**Debug → Performance** and `--profile` expose GPU geometry uploads, buffer
allocations, retained capacity, and uniform uploads, including placement upload
counts and bytes. Moving or rotating finished parts updates small transform/color
records; shape changes upload new geometry and reuse buffer capacity when it fits.
Translations also reuse cached thumbnails, bounds, and face measurements.
During geometry-changing parameter drags, display meshes use a coarser tolerance
while retaining exact-volume validation. After pointer release and 150 ms without
a parameter edit, the worker refines the mesh and finishes thumbnails using the
cached B-rep and volume. Bounds, face measurements and constraints update with
every preview. Final meshes remain cached for movement-only edits; exports
always request final preparation. See [interactive refinement](../evaluation/interactive-preview.md).
Obsolete source builds stop at checkpoints between CAD operations and meshing;
see [preview cancellation](../evaluation/preview-cancellation.md). Individual native
calls already in progress finish before cancellation takes effect.
Picking shares canonical CPU geometry and transforms queries through part poses,
avoiding world mesh and sampled-edge copies. With `--profile`, normal exit prints
slider latency median, p95, p99 and maximum over the latest 512 accepted revisions.
Timing runs from control change through GPU prepare, excluding GPU completion and
presentation. Profiling also breaks that interval into dispatch, worker queueing,
build time, result delivery and UI-to-prepare; see the
[preview scheduling investigation](../evaluation/preview-latency.md). The native
renderer explicitly requests eframe's one-frame, low-latency vsync configuration.
For an automatic, bounded test without desktop input, run
`spice-cad --profile-preview` for translation or `spice-cad --profile-rotation`
for rotation. Use `spice-cad --profile-geometry` to sweep the mounting bracket's
hole radius from 3 to 5 mm. These workloads sample control changes every 20 ms,
retain the latest pending input while the worker is busy, and report coalesced
inputs and stale worker results. They close after the final
preview and any pending thumbnails, leaving model files unchanged. See the
[slider workload guide](../evaluation/slider-workloads.md) for interpretation. The
[native Wayland baseline](../evaluation/wayland-preview.md) describes the workload
and why headless display timings can differ substantially from the desktop.
See [rotation preview measurements](../evaluation/rotation-preview.md) for the
thumbnail policy and rotation workload.
See [canonical picking measurements](../evaluation/canonical-picking.md)
for allocation checks and sustained drag results.
Numeric property edits reuse a successful type check when every other source
byte is unchanged. Runtime assertions, property bounds, and assembly checks still
run; code, metadata, and boolean changes require another type check.

# Cache and allocation audit

## Scope and measurements

2026-10-19, Linux development release, native SMLib backend. Three independent
`spice-cad --check-model FILE` invocations per bundled model, without flushing OS
caches. These are small diagnostic samples, not latency percentiles.

| Workload | Total | Kernel construction | Mesh/inspection/thumbnails/checks |
| --- | ---: | ---: | ---: |
| Four rack parts | 233–241 ms | 147–149 ms | 70–72 ms |
| Rocket engine | 215–237 ms | 30–33 ms | 173–192 ms |

The final column includes both layouts and constraint resolution; it does not
isolate tessellation. Analyzer startup/checking contributed another 11–21 ms.
This evidence favors eliminating geometry work before changing I/O scheduling.

Separate heaptrack runs of the same CLI workloads measured 974,151 allocation
calls for the rack and 839,872 for the rocket. The `smos_Calloc` entry accounted
for 842,328 (86%) and 807,083 (96%) respectively. Symbolized stacks include
SMLib curve caches and B-rep tessellation. Peak tracked heap was 6.05 MB / 6.45 MB
(decimal). These runs include process setup, checking and both scenes, exclude
the analyzer child's heap and the interactive GUI, and incur profiler overhead;
their timings are not compared with the uninstrumented samples above. Allocation
call count and retained memory measure different costs. Results are recorded in
[cache-results.json](cache-results.json).

## Ownership and invalidation

| Data | Existing cache / lifetime | Remaining cost |
| --- | --- | --- |
| Editor source/history | Rope snapshots, bounded undo history | Whole source flattened for geometry and LSP; geometry snapshot copies it again |
| Visible text | Galleys and character advances, viewport bounded | Every edit or completed syntax parse clears all visible entries; per-frame temporary vectors/maps remain |
| Syntax | Incrementally edited tree, latest pending job, cancellation | Special-character indexing scans the whole document after parsing |
| Inline results | Two scene-owned output lists, lazy texture handles | Layout switch resets textures; card anchors, keys, and placements allocate during each layout pass |
| Geometry | One current snapshot; canonical bodies discarded after building scenes | Every accepted source revision reruns all operations, including unchanged subtrees; export repeats compilation and construction |
| CPU scene | Assembly and optional Print retained together; switching swaps ownership | Each layout independently tessellates, samples edges, computes volume and inspection; named-face resolution can tessellate again |
| GPU geometry | Two lazy slots, keyed by layout and model revision | A changed revision still transfers geometry once per viewed layout |
| Camera/selection | Last 128-byte uniform payload compared by bits | Changed camera/selection requires a uniform transfer |
| Disk source | Metadata polling on background thread, one notification slot | Changed file is read by watcher and again during reload; no ordinary-frame file reads |

Geometry and syntax results are revision checked before adoption. Syntax and LSP
use replaceable pending slots; geometry has a capacity-one command/result channel
and only one active operation. Preview requests coalesce during the 250 ms debounce.
An in-flight obsolete CAD build still completes before the latest revision starts.
SMLib is protected by a global context mutex; adding geometry threads would not
establish parallel kernel execution.

## Changes in this audit

Previously each Assembly/Print switch recreated triangle and edge GPU buffers,
and each rendered frame transferred the same uniform even if nothing changed.
The viewport now retains two lazy GPU scene slots. A slot uploads only for a new
model revision. Its buffers grow geometrically within the existing vertex/edge
limits and are reused for smaller or equal-capacity rebuilds. Unchanged uniforms
do not transfer. Empty geometry does not create dummy buffers.

Maximum retained geometry capacity is 25,491,456 bytes (24.31 MiB) across both
slots, excluding uniforms, driver staging, textures and render targets. This is a
deliberate speed/memory tradeoff; the second slot allocates only when viewed.
Old revisions overwrite each slot on demand; there is no unbounded history/LRU.

Debug → Performance and `--profile` report geometry uploads, GPU buffer creations,
retained geometry capacity and uniform writes. These counters do **not** count
all Rust/C++ heap allocations or GPU completion time. Native UI validation must
check repeated toggles, rebuilds with unchanged capacity, and an alternate layout
whose cached revision is stale.

The native Vulkan/RTX 3070 check passed: initial Assembly/Print uploads created
four buffers totaling 294,912 bytes. Returning to either layout across repeated
switches kept uploads at two and allocations at four. Editing rack separation
uploaded the new Print scene once; visiting the stale Assembly slot uploaded it
once, still with four allocations. Returning to Print reused it. Camera/viewport
changes and inline part focus rendered correctly; 306 UI frames recorded 18
uniform writes. UI frames also include passes without a viewport callback, so
this is not a per-callback hit-rate measurement. All 117 workspace tests, Clippy
with all targets/features and warnings denied, and formatting passed.

## Next work, in order

First follow-up implemented: preview tessellation and edge sampling now happen
once per canonical part. Both layouts transform those vertices, normals, sampled
edges and analytic inspection data. Volume is computed once. Constraint resolution
reuses the canonical meshes, and print export transforms the canonical print mesh.
Tight layout bounds still come from native geometry rather than transformed AABB
corners. Game exports retain their distinct coarser tessellation tolerances.
All 118 tests pass, including arbitrarily rotated cylinders with tight bounds,
stable face IDs, rotated normals and preserved circle radii. One subsequent
release check measured 199 ms for the rack and 120 ms for the rocket; these are
individual diagnostic samples, not controlled before/after medians. Volume work
moved into the scene interval, so individual stage timings are not comparable
with the earlier breakdown.

1. Tessellate canonical parts once per build and derive rigidly transformed scene
   meshes/edges; reuse canonical volume. Preserve tight bounds, face IDs, normals,
   constraint coordinates, and mesh tolerances with rotated/curved regressions.
2. Retain a bounded worker-owned geometry recipe/body cache. Key operations by
   complete inputs and dependencies, including modeling tolerances and version;
   invalidate affected descendants only. Keep bodies on their owning kernel
   thread. Placement/color/comment edits should not repeat booleans. Export may
   reuse exact bodies, but game tessellation has a distinct tolerance key.
3. Reuse editor scratch capacity, cache inline placements by anchors/collapse/font
   state, retain per-layout textures, and invalidate only affected text ranges.
   Measure allocation counts and p95 editing time on large ASCII/Unicode files.
4. Send LSP document deltas when negotiated; update special-character indexes for
   changed lines. Add stale-work checks between CAD operations and before scene
   generation. An individual native kernel call may remain non-interruptible.

For each step, compare identical release workloads, record retained bytes and
allocation counts separately, and prove invalidation after edits/undo/file changes.
Interactive heap allocation profiles and input-to-presentation traces remain
unmeasured; the CLI heap samples do not establish editor allocation budgets.

## Ring buffers and TPT Torus

A bounded ring can help sustained ordered command streams or reusable upload
staging after profiling. Interactive rebuilds need latest-value semantics: a
FIFO of every slider position increases stale work. Existing single-slot queues
already bound memory; no evidence here identifies their synchronization as a
latency bottleneck. Retain sleeping workers rather than introducing busy polling.

[TPT Torus](https://github.com/tpt-solutions/tpt-torus) provides asynchronous I/O
over io_uring, IOCP and kqueue. Its published sub-nanosecond examples benchmark
API construction/inspection, not CAD work or end-to-end file throughput. It does
not cache B-reps, prevent tessellation, or manage wgpu buffers. Defer adoption
until a measured high-concurrency file/network workload warrants that dependency.

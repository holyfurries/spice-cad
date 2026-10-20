# Cache and allocation audit

## Intermediate operation retention

Preview construction now retains up to 16 intermediate B-reps in a least-recently
used cache, capped at 16 MiB of SMLib-reported allocated body memory. This cap is
separate from the existing final-part bodies, display meshes, recipe metadata,
and temporary working copies. Oversized bodies are evaluated without retention.
SMLib's `GetMemoryUsed` accounts for topology, geometry and attributes; the cloned
body is measured again before retention rather than assuming clone size matches.

Structural comparison uses the same bounded recipe matcher as the final-part
cache. Geometry inputs, profiles, edge selections and named-face selectors
invalidate affected subtrees; source offsets, names and presentation metadata do
not. Changing a later hole therefore clones the unchanged filleted predecessor
and rebuilds the cut. Cache-owned bodies never enter mutating kernel operations.

Hits charge the full original subtree evaluation cost against the 512-step model
budget. Cache capacity cannot make an otherwise over-budget model acceptable.
Failed downstream operations may leave valid predecessors cached; unsuccessful
parts never replace the previously prepared final-part cache. Eviction drops the
native body and its recipe reference. There is no unbounded revision history.

Regression tests compare a cached hole edit with uncached construction, restore
the original cut to check body isolation, enforce retention limits, and check
that cached and uncached paths consume identical logical evaluation steps.
Cold builds pay for copying retained intermediates; end-to-end slider benchmarks
should distinguish this cost from warm dependency reuse and tessellation.

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

| Data | Baseline cache / lifetime | Baseline repeated cost |
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

## Follow-up implementation

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

Second follow-up implemented: a worker-owned cache retains at most the current
32 final part subtrees, including canonical native bodies, display meshes,
sampled edges and volume. It compares full operation dependencies, numeric
inputs, profiles and edge targets without hashes; names, colors, placements and
comments are excluded. Removed parts are evicted. It deliberately does not retain
every intermediate Boolean result. Native geometry is opaque, so the 32-entry
limit is not a precise byte limit on kernel memory.

The worker keeps its serialized SMLib context for its lifetime. Preview bounds
and exports use independent native body copies; canonical cache entries never
receive placement or STEP conversion. Game meshes still use their own tolerance.
Type checking and assembly constraints rerun on cache hits. Preview source strings
move into the snapshot without a second full copy. Cold CLI exports keep their
direct construction path; native editor exports reuse the worker cache.

All 121 tests pass, including dependency invalidation, selected-edge/profile
keys, eviction, recovery after failure, changed constraints on a cache hit, and
preview/export/preview through the actual worker. Native Vulkan checks reused all
four rack parts after changing separation, measuring 27.4 ms total (16.2 ms
analysis, 1.4 ms script, 2.8 ms kernel copies, 7.1 ms scene work). This is one
warm diagnostic sample; debounce, file watching and presentation add latency.

Third follow-up implemented: visible editor rows reuse a scratch vector instead
of constructing a vector and origin map every frame. Cache retention uses direct
visible-row indexing rather than scanning visible keys for every cached row.
Inline placement layouts reuse their allocation and card capacity until anchors,
collapse state, visibility, line count or row height change. Keys are built once
per result set; alternate layout thumbnail textures survive switching.

Text edits invalidate affected rows and rebase later rows after newline changes.
Completed parses invalidate the union of edited lines and Tree-sitter structural
changes, so multiline comments refresh subsequent rows correctly. Unknown edit
chains, initial parsing and DPI/font cache changes fall back to broader
invalidation. An empty memoized galley marks the egui font-cache lifetime without
allocating a text job each frame; replacing that marker invalidates retained
galleys after atlas/font resets. The 127-test suite covers these cases,
character-by-character typing/newline splits, layout/texture storage reuse and existing
100,000-line/long-line rendering, Unicode, modal edits and horizontal scrolling.
Clippy with all targets/features and warnings denied, formatting and release
build pass. This removes identified hot-path allocations; it does not establish
that the whole egui frame is allocation-free.
Native Vulkan validation also passed an unsaved modal-editor comment insertion,
syntax refresh and automatic preview with all four parts reused; layout switching,
part focus and buffer reuse remained correct.
Shutdown closes both worker channels and joins the geometry thread, ensuring
cached native bodies are released before library teardown. A full-channel
regression verifies that shutdown disconnects both directions and waits for the
worker to finish. An in-flight native call completes before teardown.
The latest release also closed cleanly through the native editor's `:q` command
after loading the default rack and populating its persistent cache.

## Interactive parameter updates

Sliders previously committed only on release, then waited for the 250 ms typing
debounce. Parameter edits now request an immediate preview. While the worker is
busy, controls retain their latest draft values and apply the next source edit
when it becomes idle. No queue of intermediate positions accumulates. Existing
revision checks remain intact; obsolete builds are not adopted to animate a drag.
Code typing retains its debounce, and disabling Auto Preview still requires Run
or Save to rebuild.

The inspector keeps bounded parameter metadata across its own literal edits,
rebasing subsequent character ranges without waiting for asynchronous syntax
results. External edits and undo require matching parsed metadata again. A drag
merges its source edits into one bounded undo entry. The properties pane opens
by default.

Release Vulkan/RTX 3070 validation recorded 20 additional scene uploads during a
held separation drag, with GPU buffer allocations unchanged at two. A sampled
cached build took 36.4 ms (26.0 ms analysis, 1.3 ms script, 2.9 ms kernel,
6.1 ms scenes). Changing rack width also produced several uploads before release;
its final full rebuild took 200.9 ms, with all four canonical parts rebuilt.
These are individual CPU pipeline samples, not input-to-presentation latency or
frame-rate guarantees. Final saved values matched the controls; one Undo restored
the original separation. Manual mode retained the preview until Run, and an
invalid step rise preserved the previous geometry and recovered on a valid value.
Controls remained visible throughout. All 129 workspace tests, Clippy with all
targets/features and warnings denied, formatting and release build passed.

## Placement fast path

Both layouts now share one canonical GPU triangle/edge buffer pair. Vertices and
segments identify their part; vertex shaders apply that part's matrix and color.
Movement, rotation, color changes and layout switches update at most 32 records
of 80 bytes each. A four-part placement upload is 320 bytes. Geometry identity
depends on the ordered canonical parts, independently of source/preview revision.
Shape changes or part reordering invalidate that payload. Face, edge and part
selections follow unchanged geometry to its updated placement. Geometry buffer capacity
is bounded by 13,270,016 bytes (12.66 MiB), plus a fixed 2,560-byte placement buffer
and camera uniform; driver staging, CPU caches and textures are separate.

The worker retains up to two orientation/color presentations per current part.
Translation reuses thumbnails and face spans, shifts tight bounds, and never
copies a native body. Rotation refreshes orientation-dependent presentation and
obtains tight bounds from an independent native body copy. Volume and canonical
tessellation remain cached. Inline thumbnail textures retain their handles while
their shared image is unchanged. The canonical-picking follow-up below removes
CPU world meshes and sampled edge copies. Export creates independent mutable body copies.

A single successful type-check cache normalizes only validated numeric literal
spans in parsed `cad.number` property declarations. All other source bytes are
compared exactly, without hashes; boolean literals and metadata are preserved.
Luau has one numeric type and [does not support numeric singleton types](https://luau.org/types/basic-types/#singleton-types-aka-literal-types).
Malformed/unrecognized declarations remain exact bytes; a parse timeout falls back
to full checking. Only a successful check populates the cache. Runtime evaluation,
resource limits, property validation and assembly constraints always rerun.
The language server independently refreshes diagnostics for the actual source.

The release rack separation sample now measured 2.2 ms for the build pipeline:
1.3 ms analysis-key preparation, 0.8 ms script evaluation, no native construction,
and 0.1 ms scene preparation. The earlier slider sample was 36.4 ms; these are
individual CPU samples, not controlled percentiles or presentation timings.
During a held drag, geometry uploads stayed at one and allocations at two while
placement uploads increased from one to 21 (320 bytes per update). Assembly/Print
switches likewise preserved geometry buffers. Native checks cover final values,
one-step Undo, shape invalidation and picking; unit checks compare GPU transforms
with CPU positions, normals, face IDs and edges in both layouts. All 134 workspace
tests, Clippy with all targets/features and warnings denied, formatting and release
build pass, including cached print/game exports and constraint failure/recovery.

## Canonical picking follow-up

Picking now shares canonical CPU geometry with rendering and transforms rays or
projects local edge segments through each part's pose. Global occlusion remains
intact. Scene construction no longer materializes world meshes or sampled edge
arrays; orientation presentations discard temporary meshes after inspection.

[Matched measurements](canonical-picking.md) show 59 → 42 scene allocation calls
(28.8% fewer) during sustained separation dragging. Scoped tests record zero Rust
allocations across 1,024 picking queries and scene allocation bytes independent of
tessellation density. A separate hardware-rendered session measured 31.4 ms p95
from slider control change to GPU prepare across 234 accepted revisions. This
excludes GPU completion and presentation. All 139 workspace tests pass.

The [preview scheduling investigation](preview-latency.md) partitions this timing:
a diagnostic baseline spent 2.4 ms median building the preview and 27.7 ms waiting
for the UI to receive it. Sampling points into the native Vulkan presentation
path in the Xvfb environment. The worker already wakes the UI immediately; queue
and presentation-mode experiments did not establish a latency improvement.
The [native Wayland follow-up](wayland-preview.md) measured 9.163 ms p95 through
GPU preparation with an automatic, fixed-size sweep. Its matched Xvfb run measured
22.561 ms p95; worker time was nearly identical. Profile the native display path
before attributing headless delivery delays to the application's command queue.

## Remaining work

1. Profile interactive heap allocation counts and p95 editing latency on large
   ASCII/Unicode files, plus retained native geometry memory during repeated edits.
2. Consider intermediate-operation caching if changed-part rebuilds remain costly.
   Rotation-dependent inspection and thumbnails refresh when orientation changes.
3. Send LSP document deltas when negotiated and update special-character indexes
   for changed lines. Add stale-work checks between CAD operations and before
   scene generation. An individual native call may remain non-interruptible.

For each step, compare identical release workloads, record retained bytes and
allocation counts separately, and prove invalidation after edits/undo/file changes.
Interactive scene allocation calls now have a matched profile; large-file editor
budgets and input-to-presentation traces remain unmeasured. Neither the CLI heap
samples nor the scoped scene checks establish whole-editor allocation budgets.

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

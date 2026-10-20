# Editable rack measurements

The startup model is installed as `spice-rack-measurements.luau`; earlier personal
rack files are preserved. It exposes four named, linked measurements:

| Measurement | Default | Linked parameter |
| --- | ---: | --- |
| Lip height | 8 mm | `lip` |
| Usable tread depth | 53 mm | `jar_diameter` |
| Wall thickness | 3 mm | `wall` |
| Joint tip gap | 12.2 mm | `joint_clearance` |

Usable tread depth includes twice the jar spacing. The unrotated assembly's joint
tip gap includes its displayed separation: set separation to zero to see the
0.2 mm fit gap. The parameter editor shows the parameter value separately from
the measured distance. These annotations are reference measurements; they add
no new fabrication checks or claim of physical fit.

Repeated faces use `cad.face(..., {normal=..., at=..., near={x=...,y=...,z=...}})`.
The nearest matching face bounding-box center is chosen in canonical solid
coordinates. Ties within 0.001 mm remain errors. Copied face selectors include a
near point. Named selections retain their explicit measurement name through
rebuilds, including when multiple measurements share a face pair. Removing that
name clears the selection. Raw numeric face/edge selections require unchanged
geometry.

## Cache and validation

Annotation resolution uses per-part canonical plane maps, lazily cached beside
geometry. Pose and color edits reuse them; geometry changes replace the affected
entries. The preview no longer combines and copies every part's mesh to resolve
annotations. Source analysis and measurement binding validation share one fresh
parameter extraction, preserving current literal ranges after numeric edits.

All 155 workspace tests, Clippy with all targets/features and warnings denied,
formatting and the release build pass. Coverage includes selector ambiguity,
invalid coordinates, global face IDs, changed properties, three/eight-step racks,
zero-separation clearance, named selection removal, and shared-face names. The
warmed annotation allocation test records six allocations and 1,295 requested
bytes for both 372-vertex and 12,372-vertex cylinders. This measures annotation
resolution, excluding script execution, cold plane extraction and native kernel
work.

Native Xvfb interaction verified opening the new default, editing lip height
8 → 12 → 10 from its measurement label, retained face selection, synchronized
Properties/source, saving, and one undo per edit back to the original file.

## Native preview measurements

Two runs per configuration used native Wayland, NVIDIA RTX 3070 Vulkan,
1200×650 windows and 300 changes in 20 ms slots. No compilation ran concurrently.
The baseline binary predates rack annotations; candidate runs include all four.

| Metric | Before annotations | Annotated rack |
| --- | ---: | ---: |
| Translation control → GPU preparation p95 | 8.861–8.870 ms | 9.033–9.053 ms |
| Rotation control → GPU preparation p95 | 8.838–8.839 ms | 8.958–9.025 ms |
| Translation worker median | 1.958–2.018 ms | 2.601–2.676 ms |
| Rotation worker median | 4.688–4.739 ms | 5.538–5.652 ms |

Every run completed all 300 changes with one geometry upload, two GPU buffer
allocations and 147,456 bytes of geometry capacity. Translation rendered eight
initial thumbnails; rotation rendered those eight plus four after settling.
Additional measurement work consumes worker time while keeping end-to-end p95
within 0.2 ms of this baseline. These are diagnostic desktop runs, not a
statistical guarantee or physical presentation measurements.

Reproduce with `nix develop -c cargo run --release -- --profile-preview` and
`nix develop -c cargo run --release -- --profile-rotation`.

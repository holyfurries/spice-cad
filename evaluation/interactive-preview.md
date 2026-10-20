# Interactive display refinement

Geometry-changing parameter drags use 0.25 mm / 30° display tessellation.
After pointer release and 150 ms without a parameter edit, the worker replaces
that mesh with the normal 0.02 mm / 15° mesh and completes thumbnails. The
snapshot explicitly records pending geometry refinement; the slider benchmark
only finishes once its latest revision has final geometry and thumbnails.
Completion additionally requires the viewport's prepared geometry allocation
to match the final snapshot, so receiving a refined mesh is insufficient.
Its `preview_sweep_quality final=true` line exposes that completion condition.

The cache keeps one active mesh per part. A final mesh also satisfies an
interactive request, so moving or rotating unchanged geometry preserves its
mesh allocation and GPU upload. Refinement reuses the canonical B-rep, exact
volume, and tight bounds without reevaluating CAD operations. It replaces the
mesh-dependent presentation and dimension-plane caches. Print export always
calls final-quality preparation, even when a drag preview is still coarse.
Game export retains its existing independent mesh generation.

Both qualities retain closed-mesh, winding, triangle-budget, and exact-volume
checks. Exact integration still uses 1e-8; its precision is unchanged. The mesh
versus exact-volume comparison uses its existing chord-dependent tolerance.
CAD edges and bounds retain their original accuracy. Arbitrary face/edge
selections clear on remeshing, as they do on geometry edits, preventing an old
triangle-derived face ID from selecting another face. Named measurements and
part selections use the existing semantic restoration path.

Run the isolated comparison without competing builds:

```sh
nix develop -c cargo test --locked -p spice-cad --all-features \
  profile_interactive_tessellation -- --ignored --nocapture
```

Five alternating-order debug samples on the mounting bracket measured
17.8–19.6 ms for interactive mesh generation plus smoothing, versus 49–55.5 ms
for final quality: 788 versus 2,564 triangles, both with 42 CAD faces. Exact
volume was primed before these samples to isolate tessellation. The rack's
flat geometry produced the same triangle counts with either quality and
showed no useful saving. Rocket parts had substantially fewer triangles and
lower mesh times at interactive quality.

These are mesh-stage measurements, not complete slider latency. First-time
exact-volume integration and CAD construction remain necessary, and final
refinement adds an extra mesh pass after dragging stops. Full interaction
measurements should use `--profile-geometry` with the same renderer, machine,
and workload before and after the change.

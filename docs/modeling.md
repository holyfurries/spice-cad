# Modeling with Luau

[← Spice CAD](../README.md) · [Editor](editor.md) · [Printing](printing.md)

The [typed CAD declarations](../crates/spice-cad/types/cad.luau) define the API;
[examples](../examples/) provide runnable models. Use `--!strict` and annotate
factory functions for useful diagnostics and completion. Lengths are millimetres,
angles are degrees, and X/Y/Z mean width/depth/height.

## Define solids and properties

```lua
--!strict
local width: number = cad.number(80, {
    label = "Width", min = 20, max = 120, step = 1, unit = "mm",
})
local plate = cad.box({ width = width, depth = 50, height = 8 })
cad.show("Plate", cad.fillet(plate, { radius = 2 }))
```

Top-level literal `cad.number` declarations appear in Properties. Both `min` and
`max` enable a slider; otherwise the control is a numeric field. `cad.toggle`
creates a checkbox. Controls edit the source literal, and each slider drag is one
undo step. Enter or leaving a field applies its value; Escape cancels the draft.
Use ordinary locals for values that should stay private. Cross-property rules
belong in `assert` calls or explicit measurement checks.

| Operation | Usage |
| --- | --- |
| Box | `cad.box({width=40, depth=20, height=8, x=0, y=0, z=0})` |
| Cylinder, along +Z | `cad.cylinder({radius=5, height=20})` |
| Boolean union / difference | `cad.union(a, b)` / `cad.subtract(a, b)` |
| Translate | `cad.translate(part, {x=10, z=4})` |
| Rotate about the source origin | `cad.rotate(part, {axis="z", angle=90})` |
| Fillet / Chamfer | `cad.fillet(part, {radius=2})` / `cad.chamfer(part, {width=1})` |

Box coordinates specify its minimum corner; cylinder coordinates specify its base
center. Operations return new recipes, so operands can be reused. Transforms
inside a construction change geometry; finished-part placements below are cheaper.

To treat selected straight edges, create `cad.edge(part, {name=..., from=...,
to=...})` handles and pass `edges={handle}` to fillet or chamfer. Endpoints use
source-local coordinates. Select handles on the exact input solid; handles from
an earlier solid are invalid after another modeling operation.

## Profiles, extrusions, and revolutions

```lua
local section = cad.polygon({
    {x=5,y=0}, {x=12,y=0}, {x=12,y=20}, {x=5,y=20},
})
cad.show("Bushing", cad.revolve(section, {angle=360}))
```

`cad.polygon` closes a sequence of 2D vertices. `cad.circle({radius=5})` produces
a circular profile. `cad.path` combines lines and arcs: each segment has a `to`
point; arcs also specify `center` and optional `clockwise`. `cad.profile({loops=
{outer, hole}})` groups nested loops for supported extrusions. Loops must be
closed, nonintersecting, and consistently nested.

`cad.extrude(profile, {height=6})` extrudes along +Z. `cad.revolve` interprets
profile X as radius and Y as world Z, revolving from +X toward +Y around +Z.
Omit `angle` for a full turn; angles from 0.1° to 360° create capped sectors.
Radii must stay within 0–1000 mm; touching the axis is allowed, crossing it is not.
See [extruded bracket](../examples/extruded-bracket.luau),
[rounded enclosure](../examples/rounded-enclosure.luau), and
[revolved bushing](../examples/revolved-bushing.luau).

## Sweeps, lofts, and shells

```lua
local elbow = cad.sweep(cad.circle({radius=3}), {
    path = {{x=0,y=0,z=0}, {x=0,y=0,z=40}, {x=40,y=0,z=40}},
    bend_radius = 12,
})
```

Sweeps accept a polygon or full circle and 2–64 points forming an open planar
path. The profile center follows the path, perpendicular to its initial tangent;
ends are capped. Omit `bend_radius` for mitered corners. Supplying it inserts
exact tangent circular bends (G1 continuity). The radius must exceed the profile
footprint, with 0.01 mm of straight length left between bends and ends. Total
path length is limited to 4000 mm. Reversals, close nonadjacent segments, nested
profile loops, twisting, scaling, and nonplanar or freeform spline paths are
unsupported. See [smooth sweep](../examples/smooth-sweep.luau).

```lua
local body = cad.loft({
    {profile=cad.circle({radius=12}), z=0},
    {profile=cad.circle({radius=14}), z=12},
    {profile=cad.circle({radius=10}), z=28},
    {profile=cad.circle({radius=6}), z=40},
}, {degree=3})
```

Lofts accept 2–16 convex single-loop profiles in parallel XY planes. Section
heights increase by at least 0.01 mm; `x` and `y` add section offsets. Corresponding
edges, arc sweeps, and seams must match. Holes, concave sections, and transitions
between circles and polygons are unsupported. Default degree is 1; degrees 2/3
need at least 3/4 sections. Curved lofts may bulge between sections, so inspect
bounds. See [curved loft](../examples/curved-loft.luau).

`cad.shell(part, {thickness=2, open=face})` hollows inward while retaining the
outside shape. Omit `open` for an enclosed cavity, or select one axis-aligned
planar face for the opening. Thickness must be less than half the smallest bound;
local features can impose tighter limits. Shell before applying fillets when
possible. See [shape operations](../examples/shape-operations.luau).

## Named faces and design checks

```lua
local height = cad.number(20, {label="Height", unit="mm"})
local part = cad.box({width=40, depth=30, height=height})
local top = cad.face(part, {name="top", normal="z+"})
local bottom = cad.face(part, {name="bottom", normal="z-"})
cad.measure("height", {from=bottom, to=top, min=2, max=25, parameter="height"})
cad.show("Measured block", part)
```

Face selectors match an outward normal (`x+`, `x-`, `y+`, `y-`, `z+`, `z-`). Add
`at` to filter the coordinate along that axis, or `near={x=...,y=...,z=...}` to
choose the closest matching face center. Ambiguous or missing faces fail the
build. Names and selectors are reevaluated after each rebuild; numeric face IDs
are not persistent references. Named measurements require faces of final shown
solids. Select intermediate solids only when making another modeling operation.

`cad.measure` reports separation between parallel planes. Without limits it is
a reference dimension; `min` or `max` makes it a design check that blocks export
on failure. An optional `parameter` names a top-level numeric control for editing
from a dimension label. This association does not assert that the parameter
equals or drives the dimension, and it does not invert formulas.

Measurements use the preview mesh, display `≈`, and allow 0.001 mm plane/check
tolerance. They are bounded preview checks rather than exact kernel metrology.
Up to 64 named faces and 64 scripted measurements are supported. Deterministic
code does not prove fit, strength, or manufacturability; there is no sketch
constraint solver or automatic fully-constrained classification.

## Face-based features

```lua
local part = cad.box({width=60, depth=30, height=8})
local top = cad.face(part, {name="mounting face", normal="z+"})
part = cad.hole(part, {face=top, radius=2, u=-20, v=0})
cad.show("Mount", part)
```

Holes cut inward; omit `depth` for a through-hole or supply it for a blind hole.
Offsets start at the selected face's bounding-box center. X-normal faces use
+Y/+Z for `u/v`; Y-normal faces use +X/+Z; Z-normal faces use +X/+Y. The axes are
the same for either normal sign. Centers outside the trimmed face or inside an
opening fail; a circle may overlap a boundary to create an open cut.

Add `counterbore={radius=4,depth=2}` or `countersink={radius=4,angle=90}` for a
screw seat. Recess radius must exceed hole radius; countersink angles are included
angles. Select a fresh face from the resulting solid before another operation.
`cad.pocket` cuts inward and `cad.boss` adds outward, using the same face axes.
Supply `width`/`height` or a polygon `profile`, plus `depth` and optional `u/v`.
See [manufacturing panel](../examples/manufacturing-panel.luau).

## Part placements and assemblies

```lua
local plate = cad.box({width=30, depth=20, height=4})
cad.show("Plate", plate, {
    assembly = {position={z=40}},
    print = {rotation={x=90}, position={y=4}},
})
```

Use unique `cad.show` names. Rotations apply about the solid's source origin in
X/Y/Z order, followed by translation. Omitted assembly placement means identity.
If any part declares `print`, every part needs one (`print={}` means identity).
Without print placements, print exports use assembly placements. Changing finished
part placements reuses geometry; switching layouts changes transforms only.

Selectors use local geometry coordinates. Scripted checks use assembly coordinates,
even in Print layout; direct selection measures the displayed placement. Print
exports use print placement, while game exports use assembly placement. Visibility
only affects the viewport. The model supports at most 32 named parts.

## Local modules

```lua
-- main.luau
local bracket = require("./bracket")
cad.show("Bracket", bracket({width=30}))
```

The entry file's directory is the project root; no manifest is needed. Literal
relative imports resolve from their importer, with an optional `.luau` extension.
Nested imports may use `./../` inside the root. Absolute paths, packages, dynamic
imports, cycles, and symlinks escaping the root are rejected. Projects have at
most 64 files and 4 MiB combined source.

Modules return a value, commonly a typed factory function. They can use `cad`
directly and share the execution budget. An executed `require` runs the module
once; repeated imports share its result. Keep controls and `cad.show` calls in
the entry file. Exported types and function annotations work across modules.

Imported edits invalidate analysis and preview caches. Exports include staged
module files with rewritten import paths; keep these with `model.luau` to rebuild
the exported snapshot. **File → Examples → Reusable typed modules** installs the
[modular example](../examples/modular/main.luau) without overwriting personal edits.

## Exports and headless commands

```sh
cargo run --release --locked -- --check-model examples/mounting-bracket.luau
cargo run --release --locked -- --render-views examples/rack-parts.luau output/views
cargo run --release --locked -- --export-game examples/rocket-engine.luau output/engine
```

Run Cargo from `nix develop`. Output directories must be new. Game export writes
`model.glb`, the source graph, and `GAME_ASSET.md`. GLB uses separate named objects,
part colors, and hard boundaries between CAD faces. Coordinates convert to
right-handed Y-up metres: `(x,y,z)` becomes `(x,z,-y)/1000`. The source origin
remains the pivot. It has no print-bed requirement; closed meshes and declared
checks still apply.

Game tessellation uses 0.25 mm chord / 30° angular tolerances, with no fixed polygon
budget or automatic LOD generation. UVs, textures, collision meshes, rigging, and
animation are unsupported. [Printing](printing.md) uses finer tessellation and
separate bed placements.

## Execution limits

The editor accepts up to 16 MiB of UTF-8. Luau execution has an 8 MiB VM budget
and a 250 ms script budget; native operations have their own geometric limits.
Coordinates stay within ±2000 mm. Complex kernel operations may fail even within
input limits. Previous valid previews remain visible after errors, but export
requires a successful build of the current source.

# Modeling and exports

[← Spice CAD](../README.md) · Commands run from the repository root.

## Part placement

Declare part placements alongside the geometry, instead of branching the
geometry on a layout boolean:

```lua
local plate = cad.box({ width = 30, depth = 20, height = 4 })
cad.show("Plate", plate, {
    assembly = { position = { z = 40 } },
    print = { rotation = { x = 90 }, position = { y = 4 } },
})
```

Rotations are degrees about the solid's source origin, applied X, then Y, then
Z; translation follows in millimetres. Omitted assembly placement means
identity. If any part declares a print placement, every part must declare one
(`print = {}` means identity). Without print placements, exports use the
assembly placement. Both layouts share canonical GPU geometry; switching layouts
updates part transforms without kernel work or another mesh upload. Source edits
retain the active layout. Placing a finished part through `cad.show` reuses its
geometry. Transforms inside a Boolean construction remain geometry dependencies.

Named face selectors use the original geometry's coordinates. Design checks run
in assembly coordinates, including rotated and translated parts. Print-layout
inline measurements are labeled **Assembly**; selecting geometry still measures
its actual print placement. Copied face/edge selectors convert back to source
coordinates. Game exports always use assembly placements; print exports always
use print placements. Print layouts are arrangements of separate parts, which
should be sliced separately.

## Inline results

The code pane's **Results** toggle shows collapsible output cards below executed
`cad.show` and `cad.measure` calls, including multiline calls and calls inside
loops. Part cards contain a static shaded thumbnail, X/Y/Z dimensions, and exact
CAD volume. Measurement cards show their distance and whether declared limits
passed. Click a card header to collapse it. Click a part thumbnail or its
statistics to select and frame that part in the viewport, preserving the
camera's viewing direction. Code-only view switches to Split view; the part is
highlighted as a whole. Results stay marked **previous result** after edits
until a successful rebuild; edits whose source locations cannot be tracked hide
the cards until then. Previous results cannot select geometry until a successful
run. Thumbnails are generated on the CAD worker with a fixed work budget; dense
models can show a viewport fallback while retaining their dimensions and volume.
Cards are virtualized with the source and are never saved into the Luau file.

## Solids and edge operations

`models/mounting-bracket.luau` has rounded edges and four mounting holes, with
parameter assertions that keep holes clear of walls. `models/bracket.luau`
demonstrates box booleans; `models/rack.luau` recreates the rack blank in code.
All coordinates and dimensions are millimetres. `cad.box` takes `width`,
`depth`, `height`, and optional minimum-corner coordinates `x`, `y`, `z`
(default zero). `cad.union(a, b)` and `cad.subtract(a, b)` return new solid
recipes; operands can be reused. Call `cad.show("name", solid)` for each named
part. Ordinary Luau functions, arithmetic, and loops work.

`cad.cylinder({ radius, height, x?, y?, z? })` creates a cylinder along +Z, with
its base centre at the supplied coordinates.
`cad.translate(solid, { x?, y?, z? })` offsets a solid;
`cad.rotate(solid, { axis = "x", angle = -90 })` rotates it about a global axis
through the origin, in degrees. Translate after rotating when placing a hole.
`cad.fillet(solid, { radius = 1 })` applies circular fillets to all eligible
edges of that solid when `edges` is omitted. To round specific straight edges,
use named references and an explicit edge list:

```lua
local part = cad.box({ width = 40, depth = 20, height = 8 })
local corner = cad.edge(part, {
    name = "front_right",
    from = { x = 40, y = 0, z = 0 },
    to = { x = 40, y = 0, z = 8 },
})
cad.show("Rounded corner", cad.fillet(part, { radius = 2, edges = { corner } }))
```

**File → Examples → Selected edge fillets** rounds two plate corners while preserving
the mounting hole rims. Select a straight edge and click **Copy edge selector**
in Properties to copy the `cad.edge(...)` expression. Replace `part` with the
solid variable being filleted and give the reference a unique name. Use
parameter expressions for endpoints that should follow dimension edits; copied
coordinates are literals. Endpoint order is irrelevant.

References resolve on their source solid when used, matching a complete straight
edge within 0.001 mm. Missing or ambiguous matches, duplicate selections, and
handles from a different solid fail explicitly. Define references after booleans
and transforms, before the fillet consuming them. There are at most 64 named
edge references and 1–64 selected edges per fillet; an empty list is rejected.
These references do not track arbitrary topology changes. Circular-edge modeling
selectors remain future work.

Use `cad.chamfer(part, { width = 2, edges = { corner } })` to bevel the selected
straight edges. **File → Examples → Selected edge chamfers** preserves sharp hole rims
while beveling two plate corners. `width` is measured across the flat bevel
face, not along either neighboring face: at a right-angle corner, a 2 mm bevel
has approximately 1.414 mm setback on each face. Omit `edges` to chamfer all
eligible edges. Widths must be finite and 0.01–1000 mm; whole-solid operations
also require a size smaller than half the solid's smallest bounding dimension.
Use new edge references on each resulting solid when combining fillets and
chamfers.

## Extrusions and revolutions

**File → Examples → Extruded bracket** demonstrates a concave polygon profile, a
diagonal web, and mounting holes. `cad.polygon(points)` snapshots a closed 2D
outline; list each vertex once, in either winding order.
`cad.extrude(profile, { height })` creates a capped solid from Z=0 along +Z.
Profiles can be reused at different heights; existing booleans, fillets, and
transforms operate on the resulting solid.

```lua
local outline = cad.polygon({
    { x = 0, y = 0 },
    { x = 40, y = 0 },
    { x = 40, y = 8 },
    { x = 8, y = 8 },
    { x = 8, y = 30 },
    { x = 0, y = 30 },
})
cad.show("L plate", cad.extrude(outline, { height = 6 }))
```

Each polygon accepts 3–128 vertices with finite coordinates within ±2000 mm,
width/depth at most 1000 mm, and edges at least 0.01 mm long.
Self-intersections, touching boundaries, repeated closing points, and
overlapping edges are rejected. Extrusion height is 0.01–1000 mm; models may
define at most 256 profiles. Polygon outlines use straight segments; the APIs
below add curves and nested holes. Interactive sketch constraints remain future
work.

### Curved profiles and nested loops

`cad.circle({ radius = 10, x = 0, y = 0 })` creates an exact circular profile.
`cad.path` joins straight segments and exact circular arcs in the XY plane:

```lua
local outline = cad.path({
    start = { x = 0, y = -5 },
    segments = {
        { to = { x = 20, y = -5 } },
        { to = { x = 20, y = 5 }, center = { x = 20, y = 0 } },
        { to = { x = 0, y = 5 } },
        { to = { x = 0, y = -5 }, center = { x = 0, y = 0 } },
    },
})
local section = cad.profile({ loops = { outline, cad.circle({ radius = 2, x = 10 }) } })
cad.show("Slotted plate", cad.extrude(section, { height = 4 }))
```

An entry with `center` is a counterclockwise arc; add `clockwise = true` to
reverse it. Both endpoints must have the same radius from the center. The last
endpoint must equal `start`; use `cad.circle` for a full circle. Paths accept
2–128 segments, and profile regions accept 1–16 simple, non-touching loops.
Coordinates must be finite and within ±2000 mm, with extents no larger than
1000 mm. Circle radii are 0.01–500 mm. Profiles snapshot their input tables.

List all outer boundaries, holes, and islands together in `loops`; containment
determines nesting independent of input order or winding. Even nesting depths
add material and odd depths remove it. Intersections, overlaps, touching loops,
and self-intersecting paths are rejected before kernel construction. Arc
boundaries and intersections use analytic geometry; the displayed mesh is
tessellated separately. Extrusion and face-based pockets/bosses accept these
profiles; revolve currently requires a straight-sided polygon.

`models/rounded-enclosure.luau` demonstrates an editable enclosure with exact
rounded corners, nested wall boundaries, and a floor with four screw holes.
Export it with `--export-model models/rounded-enclosure.luau output/enclosure`.

### Revolving polygon profiles

**File → Examples → Revolved bushing** demonstrates a flanged part with a through-bore.

`cad.revolve(profile, { angle = 360 })` interprets X as radius and Y as height,
maps the section into the XZ plane, and turns it around +Z. For example:

```lua
local section = cad.polygon({
    { x = 5, y = 0 },
    { x = 12, y = 0 },
    { x = 12, y = 20 },
    { x = 5, y = 20 },
})
cad.show("Bushing", cad.revolve(section, { angle = 360 }))
```

Use `{}` for the default full turn, or set `angle` between 0.1° and 360° for a
capped sector. Positive angles start along +X and turn toward +Y. Radius must be
0–1000 mm throughout the profile; touching the axis is allowed, crossing it is
rejected. Profile Y becomes world Z, including negative heights. Reuse profiles
or apply existing transforms and booleans to the resulting solids. Export
retains the usual P1S envelope and Z=0 placement checks.

```lua
--!strict
local width: number = 100
local base = cad.box({ width = width, depth = 60, height = 6 })
local slot = cad.box({ width = 20, depth = 10, height = 8, x = 40, y = 25, z = -1 })
cad.show("slotted plate", cad.subtract(base, slot))
```

## Headless validation and images

Check a model without opening a window:

```sh
nix develop -c cargo run --release --locked -p spice-cad -- --check-model models/mounting-bracket.luau
```

Render inspection images without a display server or GPU:

```sh
nix develop -c cargo run --release --locked -p spice-cad -- --render-views models/rack-parts.luau output/rack-views
```

This creates `top.png`, `front.png`, `side.png`, and `isometric.png` at 800 × 600,
a labeled 1600 × 1200 `contact-sheet.png`, and the exact `model.luau` source.
Views use the assembly layout, part colors, shaded faces, and visible CAD edges.
Top looks down −Z with +Y up; front looks along +Y with +Z up; side looks along
−X with +Z up. Isometric looks from (+X, −Y, +Z). All views are orthographic and
fit independently, so their image scales can differ. These are visual inspection
images; use the in-app measurements for dimensions. Source, geometry, and declared
measurement checks run before writing. Existing directories are preserved, and a
failed write removes the newly created directory. Rendering uses a bounded CPU
rasterizer shared with inline thumbnails; it does not initialize a window or GPU.

## Print export

Choose **File → Export → 3D print · STEP + STL** to create a new
`output/native/model-<timestamp>/` directory. Or choose a new directory from the
command line:

```sh
nix develop -c cargo run --release --locked -p spice-cad -- --export-model models/mounting-bracket.luau output/mounting-bracket
```

The directory contains `model.step`, `model.stl`, the exact `model.luau` source
snapshot, and `PRINTING.md`. Existing directories are preserved. The exporter
checks mesh closure/orientation, the P1S's 256 mm envelope, and placement on Z=0
before writing. Import the STL into Bambu Studio in millimetres; inspect
supports and orientation before slicing. Strength, fit, and support needs are
not physically verified. The script export uses native SMLib throughout. STEP
preserves topology and uses the kernel's rational NURBS representation rather
than triangulated surfaces; analytic surfaces are converted to NURBS, and some
intersection curves may be fitted at the kernel's tolerance. Conversion must
preserve volume within `1e-5` relative plus `0.001 mm³`, or export is cancelled.
STL uses a 0.02 mm chord tolerance and passes the same closed-mesh audit as the
viewport.

## Game export

For static game props and vehicle parts, choose **File → Export → Game asset · GLB** or
use the command palette's **File: export game asset GLB**:

```sh
nix develop -c cargo run --release --locked -p spice-cad -- --export-game models/revolved-bushing.luau output/game-bushing
```

This creates `model.glb`, the exact `model.luau`, and `GAME_ASSET.md` in a new
directory. The GLB contains an indexed triangle mesh with smooth normals within
CAD faces, hard boundaries between faces, and a neutral material. It uses
coarser 0.25 mm chord / 30° angular tessellation to reduce curved-surface
triangle counts; this is not a fixed polygon budget or automatic LOD generation.
Printing retains its finer tessellation and native solid geometry.

Game exports follow
[glTF 2.0](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html): metres,
right-handed coordinates, Y up. CAD `(x, y, z)` becomes `(x, z, -y) / 1000`. The
source origin stays the pivot, including negative coordinates; game assets have
no P1S envelope or bed-placement requirement. Closed-mesh validation and
declared dimension checks still apply. Each named part exports as a separate
object with its own color material. UVs, textures, collision meshes, rigging,
and animation remain future work. These are static hard-surface asset workflows.

Use **File → Examples → Rocket engine assembly** to try a three-part asset. Its toolbar
layout switch shows the assembly or each part on Z=0; no source edit is needed
for print export. The model is a static visual asset.

## Assemblies

Multiple `cad.show` calls build an assembly without fusing its solids:

```lua
local base = cad.box({ width = 40, depth = 30, height = 4 })
local cap = cad.box({ width = 20, depth = 20, height = 6, x = 10, y = 5, z = 4 })
cad.show("Base", base, { color = { r = 0.25, g = 0.35, b = 0.65 } })
cad.show("Cap", cap, { color = { r = 0.85, g = 0.40, b = 0.10 } })
```

Part names must be unique, with 1–80 bytes and no control characters. Each
output must reference a distinct solid recipe; translate a copy when repeating a
part. Optional RGB channels use linear values from 0 to 1. Colors appear in the
viewport and GLB materials; STEP/STL carry geometry only. The properties pane
lists the parts and identifies the owner of a selected face. Measurements can
reference named faces on different shown parts. Touching or overlapping parts
remain independent; the reported assembly volume is the sum of part volumes.

Assemblies share the existing 256-operation, 512-evaluation-step, and
100,000-triangle budgets, with up to 32 parts. Print exports check every part
before writing anything: each must fit the P1S envelope and rest on Z=0 in the
source. A single part keeps `model.step` / `model.stl`; multiple parts use
`part-01.step` / `part-01.stl`, etc. `parts.json` maps these safe filenames to
source names. Arrange the separate files on slicer plates as needed; no packing,
auto-orientation, or support generation is performed.

## Language services and editor behavior

The host runs `luau-analyze` in strict mode against the exact source snapshot
and the API type context in `native/types/cad.luau` before executing it with
`mlua`. The Nix analyzer and embedded VM are both pinned to Luau 0.740. The
persistent `luau-lsp` server checks types after a 350 ms typing pause,
independently of geometry building. Superseded checks are cancelled and stale
results are discarded. The resizable **Diagnostics** panel shows errors and
warnings for the current document; click a line/column link to reveal and select
its source range. Located runtime failures also link to the failing line. The
status-bar issue indicator hides or restores the panel. Ctrl+Space opens
completion; Enter or Tab accepts the selected item. F1 or Normal-mode `K` shows
type information; F12 or `gd` goes to a definition. The server includes the CAD
API declarations. Tree-sitter parses Luau incrementally on a separate worker,
including multiline strings and comments; the UI highlights only the visible
text. Diagnostics use model-relative positions, including Unicode-aware
selection. Scripts build recipes before any SMLib operations execute, and failed
saves keep the previous valid preview. Models run on the geometry worker; camera
interaction continues during analysis and rebuilding. A background watcher
checks source every 250 ms and wakes the UI only when the content or read-error
state changes. Live preview uses a 250 ms debounce for source edits, including
external changes; the initial load and explicit Run/Save have no debounce delay.
Unchanged files do not schedule viewport redraws. Worker completion, input, and
window events wake the UI independently. Unchanged file metadata avoids repeated
whole-file reads. The editor uses a Ropey B-tree buffer with structurally shared
undo snapshots, bounded to 1,024 edits and 32 MiB of changed text. Dirty state
and preview eligibility use revision identities rather than per-frame text
comparisons. Only visible rows and columns are shaped and highlighted; very long
individual lines are horizontally virtualized too. Standard-mode undo/redo uses
Ctrl+Z, Ctrl+Shift+Z, or Ctrl+Y; Vim mode uses `u` and Ctrl+R. The workspace keeps camera controls over the viewport,
appearance controls under **View → Appearance**, selection and geometry in the
optional Properties pane, and open/save/export actions under **File**. Workspace
layouts and **Run** occupy the document bar below the menus. A fixed viewport
header groups Assembly/Print, Frame, camera views, and display settings. The
footer keeps diagnostics and build messages left, with editor position right. Full build
errors are available through **Details**. **View → Performance** opens a separate
window that separates analysis, script, kernel, and display preparation times;
its mesh timing includes mesh validation, smooth normals, and CAD edge sampling.
The headless check reports mesh/audit time without viewport preparation. Upload
CPU measures buffer creation; **Save → upload** uses the file's modification
time through buffer creation, including polling, debounce, and rebuild. It is
shown after saved edits, and excludes GPU completion and presentation. The first
load and unsaved draft runs have no save-latency measurement. Volume displayed
for scripted models is the kernel's solid volume; curved mesh volume is an
approximation.

## Interface conventions

Native interface colors, typography and control sizing live in
`native/src/interface.rs`. Main controls use a shared 36-point height, four-point
corners, and subtle borders; viewport and inspector controls use 28-point rows. Blue marks primary actions, selection, and focus;
secondary labels use muted text. The Properties pane puts selection details
first, followed by a bounded part list, source parameters, and geometry/check
summaries. The source starts directly below the document bar. **View → Inline results**
toggles code previews. Editor text remains 14-point monospace. Keep new controls on these defaults instead of
adding local themes. The grouping and neutral surfaces draw from Plasticity's
[interface overview](https://doc.plasticity.xyz/plasticity-essentials/plasticity-interface/user-interface-overview),
with larger text and controls for this code-based workspace.

## Resource limits

Editing, saving, and execution accept UTF-8 sources up to 16 MiB. Analysis has a
5-second request deadline. The background language server has a 1 GiB
address-space limit on Linux and bounded 64 MiB protocol messages. Run's
independent analyzer uses one thread, a 512 MiB limit and a 64 KiB output
budget. The UI retains at most 200 diagnostics. VM memory remains limited to 8
MiB, script execution to 250 ms, recipes to 256 operations, and geometry
evaluation to 512 traversal steps. These are script/evaluation limits, not a
wall-clock timeout on native kernel calls. Primitive sizes and fillet radii are
0.01–1000 mm; supplied coordinates and translations must stay within ±2000 mm.
Rotation angles are bounded to ±360000 degrees. Mesh volume checks allow a
surface-area/chord tolerance for curved tessellation; closure and winding checks
remain mandatory. The scripting environment exposes no general file/process
access. [Local typed modules](luau-projects.md) use bounded, captured imports
within the project directory. Validated geometry and meshes are reused when
their inputs match. Interactive sketch editing and per-part pivots are not
implemented. The existing P1S rack
export remains available in the rack-tools build with `--rack`.

Format source with `cargo fmt --all`, Luau files with
`stylua --syntax Luau models native/types`, and Nix with `nixfmt flake.nix`, all
inside `nix develop`. Original Luau integration checks are in
`evaluation/luau-results.json`. Current curved-model and native export checks
are in `evaluation/bracket-results.json`. Embedded editor and viewport checks
are in `evaluation/editor-results.json`. The rope buffer, visible-text rendering
path, and reproducible large-file benchmark are described in
[editor performance](../evaluation/editor-performance.md). Background checking,
diagnostic navigation, and resource budgets are described in
[the diagnostics workflow](../evaluation/diagnostics.md).

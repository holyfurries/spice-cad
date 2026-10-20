# Spice rack

A parametric CAD model written in Rust, using the open-source
[opencascade-rs](https://github.com/bschwind/opencascade-rs) bindings and
[OpenCascade](https://github.com/Open-Cascade-SAS/OCCT) geometry kernel. Model
generation and export run from the command line; no GUI modeling is required.

## Cabinet measurements

| Measurement                                   |                                                   Size |
| --------------------------------------------- | -----------------------------------------------------: |
| Cabinet interior width                        |                                                 493 mm |
| Cabinet interior depth                        |                                                 293 mm |
| Cabinet interior height                       |                                                 322 mm |
| Hinge intrusion into the opening              |                                    Approximately 22 mm |
| Hinge height above the cabinet floor          | Approximately 65 mm; measurement reference unconfirmed |
| Current spice rack width                      |                                                 260 mm |
| Width currently remaining for sauces and oils |                                   Approximately 233 mm |
| Target replacement spice section width        |                                   Approximately 310 mm |
| Width remaining beside the replacement        |         Approximately 183 mm, before gaps or a divider |
| Current spice rack levels                     |                                                      3 |
| Current rack arrangement                      |                           Steps rising toward the back |
| Largest spice jar diameter                    |                                    Approximately 50 mm |
| Largest spice jar height, including lid       |                                   Approximately 135 mm |
| Cinnamon-sugar container footprint            |                               Approximately 90 × 90 mm |
| Cinnamon-sugar container height               |                                    Approximately 70 mm |

The replacement should use more of the available height within a spice section
approximately 310 mm wide, leaving the rest for sauces and oils. The prototype
has five rising steps across that width. The square cinnamon-sugar container
stays in the sauces section and is excluded from the rack model. The rack will
be 3D printed on a Bambu Lab P1S; the filament has not yet been specified.

The hinge is a local obstruction. Sliding a low base below it may be possible,
but the taller portions, jars, insertion path, and open door must also clear it.
Its full height and depth footprint still need measuring before modeling a
cutout.

The exported rack is **310 × 283 × 179 mm**, excluding its contents. Its highest
jar platform is 171 mm above the floor. A 135 mm jar reaches 306 mm, leaving 16
mm below the cabinet ceiling. Lifting it 8 mm over its retaining lip leaves 8 mm
of ceiling clearance. The model allows **138 mm jar height**, including that
lift and a minimum 5 mm ceiling gap. Hinge clearance and access around
neighboring jars still need checking.

## Generate the model

```sh
cd ~/projects/spice-rack
nix develop
cargo run --locked -p spice-rack
```

The first run downloads the pinned environment and compiles the Rust
dependencies. Later runs reuse the build. Alternatively, run
`nix develop -c cargo run --locked -p spice-rack` without entering a shell.

Exports land in `output/`:

- `spice-rack.step`: assembled CAD geometry for inspection in FreeCAD or other
  CAD software.
- `spice-rack-{front,rear}-{left,right}.stl`: four rack parts, already oriented
  for printing.
- `spice-rack-fit-{width,depth}-{male,female}.stl`: two pairs of small joint fit
  samples.
- Each part and sample also has a STEP file in its print orientation.

STL is unitless; import as millimetres. The generator removes the old
single-piece `spice-rack.stl` after writing the new exports. Print the named
part STLs rather than the assembled preview.

STEP carries the resulting geometry. The parameters and construction logic
remain in the Rust source, rather than a GUI feature history.

## Adjust the design

Edit `PARAMETERS` near the top of `src/lib.rs`, then run
`cargo run --locked -p spice-rack` again. All lengths are millimetres. X is
width, Y is depth from front to back, and Z is height.

| Parameter              | Default | Meaning                                                                  |
| ---------------------- | ------: | ------------------------------------------------------------------------ |
| `rack_width_mm`        |     310 | Total rack width                                                         |
| `step_count`           |       5 | Number of rising steps                                                   |
| `step_rise_mm`         |      42 | Height difference between steps                                          |
| `jar_diameter_mm`      |      50 | Widest jar diameter, including its lid                                   |
| `jar_height_mm`        |     135 | Measured jar height, including its lid                                   |
| `clearance_mm`         |     1.5 | Gap between jars and around jar footprints                               |
| `cabinet_clearance_mm` |       5 | Minimum spare depth and height; also checked against total cabinet width |
| `thickness_mm`         |       3 | Platform, side, base, and riser thickness                                |
| `lip_height_mm`        |       8 | Retaining lip height above each platform                                 |

Each tread is 56 mm deep, with 53 mm clear behind its front lip. The rack fits
five 50 mm jars per step, for **25 largest-size jars**. Smaller jars may allow
more capacity. Jar geometry is omitted from exports.

The base, stepped side panels, platforms, and risers form a hollow structure
under the raised treads. The spice jar height is checked, including room to lift
vertically over the lips. Dimensions exceeding the cabinet allowances cause an
error before export.

The rack is split halfway across its width and after the third step. Its four
parts use integral dovetails in the base, platforms, and outer side panels. The
assembled dimensions and jar capacity stay the same. `src/printing.rs`
constructs the joints, splits the geometry, and positions each part on its outer
side panel for printing. `build_rack` constructs the unsplit geometry.

The model checks finite dimensions, bounded step counts, jar footprints, and the
cabinet envelope before invoking the kernel. The hinge is recorded above but not
modeled because its full footprint is unknown. Change `build_rack` when the
arrangement itself needs to change.

## Print and assemble on the P1S

The [P1S build volume](https://bambulab.com/en-us/p1p) is 256 × 256 × 256 mm.
The generator limits each print footprint to 246 × 246 mm, leaving at least 5 mm
on each side when centered on the plate. With the default dimensions:

| Part        | Print dimensions, X × Y × Z |
| ----------- | --------------------------- |
| Front left  | 95 × 174 × 161 mm           |
| Rear left   | 179 × 115 × 161 mm          |
| Front right | 95 × 174 × 155 mm           |
| Rear right  | 179 × 115 × 155 mm          |

1. Print both male/female fit pairs using the intended filament profile. Keep
   their exported orientations; the width pair tests a joint printed vertically
   and the depth pair tests one printed flat. A brim can help the narrow width
   samples.
2. Dry-fit the samples by sliding across their plate thickness. Adjust
   `JOINT_CLEARANCE_MM` in `src/printing.rs` and regenerate if needed. Its
   default is 0.2 mm enlargement of the pocket profile. The samples should slide
   together without forcing or cracking; actual fit depends on material and
   calibration.
3. Import each rack STL into Bambu Studio at 100% scale and center it on the
   plate. Keep the exported orientation, with the broad outer side panel on the
   bed. The geometry is designed for supports off; tests check all above-bed
   faces against a 45° overhang limit. Check the sliced preview before printing.
   Filament and final wall/infill settings have not yet been selected.
4. Turn the parts upright. For each left/right half, align the front section's
   two side-panel dovetails with the rear section's pockets. Start the rear
   section 3 mm outward from its seated position, then slide it inward across
   the side-panel thickness until the outside faces align. These joints slide
   sideways, rather than pushing front-to-back into the dovetail heads.
5. Lower the completed right half vertically onto the left half, engaging the
   dovetails in the base and raised platforms. All platform tops should be
   flush. Dry-fit the whole rack before any permanent bonding.

The CAD checks do not establish printed load capacity. Check cabinet insertion,
hinge clearance, and platform deflection under the intended jars before use.
Parameter changes must leave the front side panel at least 36 mm tall for its
two dovetails; the generator reports an error otherwise.

## Check changes

```sh
cargo fmt --all
nix fmt flake.nix
cargo test --workspace --locked
cargo clippy --workspace --locked --all-targets -- -D warnings
```

Tests check invalid parameters and cabinet collisions, verify closed and
consistently oriented assembled meshes in two configurations, and intersect all
25 jar positions and their vertical lifting volumes against the assembled rack
to prove clear space. They also check every part/sample mesh, print dimensions,
overhang angles, joint clearance, and sampled assembly movements for collisions.

## Native editor

Spice CAD is a [Holy Furries Studio](https://holyfurries.com) tool. The startup
splash fills the application window with the studio's paper background. Spice
CAD's geometric spice-jar mark appears in the header and splash, with the fox-and-halo
studio signature below. **Help → About Spice CAD** reopens the identity card.
The reusable vector logo is `native/assets/spice-cad.svg`.
Startup branding disappears when the first valid model is ready; it adds no
minimum display time, asset downloads, or image-decoder dependencies.

Build and launch the packaged application without a development shell:

```sh
nix run .#spice-cad
```

To install it, run `nix profile install .#spice-cad`, then launch `spice-cad`.
The package includes its analyzer and runtime dependencies and installs a
desktop entry. The first build compiles the application; subsequent launches run
the installed executable. This is a Nix package, not a portable static release.

The native application is the workspace default. From a `nix develop` shell,
`cargo run` launches its window. Use release mode for interactive modeling:

```sh
nix develop -c cargo run --release --locked
```

It uses egui/wgpu and native SMLib, with no browser, WebView, or WASM runtime.
Launch without arguments for the Luau workspace and the four-part spice rack;
use `-- --model FILE.luau` to open another model. The normal executable links
SMLib without OpenCascade. The optional rack-tools build provides the older rack
configurator and kernel comparison:

```sh
nix run .#spice-cad-rack-tools -- --rack
```

From the development shell, use
`cargo run --release --locked -p spice-cad --features rack-tools -- --rack`. The
original rack export CLI is explicitly selected with
`cargo run --locked -p spice-rack`. The starter source is embedded in the
executable and created once at
`$XDG_DATA_HOME/spice-cad/models/spice-rack-measurements.luau`, or under
`~/.local/share/spice-cad/models/` when `XDG_DATA_HOME` is unset. Existing edits
are preserved; installed applications do not need the source checkout.
**File → Examples → Spice rack · four parts** opens four named, colored rack pieces
with the same dovetail dimensions and 0.2 mm clearance as the dedicated rack
exporter. Set `separation = 0` to inspect the assembly, or increase it to expose
the joints. Use **Assembly / Print layout** with the viewport tools to inspect either
arrangement. **File → Export → 3D print · STEP + STL** always exports the four
bed-oriented parts, regardless of the displayed layout. `parts.json` maps the
numbered files to their part names; slice each part separately. The original
**Spice rack** example remains an unsplit blank. Examples open through the
normal unsaved-change guard and preserve existing personal copies; the new
source is `models/rack-parts.luau` in this checkout. The first build compiles
the pinned SMLib sources and desktop dependencies; use release mode for
interaction and performance measurements. The native package requires Rust 1.98,
supplied by the pinned Nix environment.

Drag to orbit, Shift-drag or right-drag to pan, scroll to zoom, and click a face
to select it. Press F outside the code editor to frame the model. Neutral studio
shading and dark CAD edge outlines make curved faces and boundaries easier to
read. **Surface edges** toggles outlines; **Width** adjusts their thickness.
Outlines follow kernel curves, hide occluded edges, and omit triangle diagonals
and periodic surface seams. Curved faces use smooth shading across their mesh
facets. Dimension edits in the rack configurator rebuild on a dedicated geometry
worker after a short debounce. Camera movement and selection reuse the GPU
buffers; validated jar-height and cabinet-gap changes reuse geometry. Invalid
edits keep the previous valid preview and disable printing export. The UI CPU
timing under **Performance** excludes GPU rendering and presentation; it is not
a total-frame or input-latency measurement.

In the rack-tools build, **Export P1S parts** writes four rack parts, four fit
samples, and an assembled STEP to `output/native/`. Print parts are checked for
closed, oriented meshes, P1S bounds, and above-bed overhangs before files are
written. This export uses the existing OpenCascade implementation; the native
SMLib viewport currently shows the unsplit rack blank. Joint clearance remains
0.2 mm in the native app. Native project files, interactive sketches, feature
history, and edge editing are future work.

`src/lib.rs` owns shared rack parameters and the OpenCascade reference model;
`src/main.rs` remains the export CLI. `native/src/kernel.rs` contains the SMLib
binding and mesh extraction, `viewport.rs` the persistent GPU renderer, and
`mesh.rs` mesh checks and face picking. SMLib handles stay on one worker thread;
its global context is serialized. Mesh extraction includes cavity shells and
corrects their winding using the source regions' void flags.

To compare native kernels on the default rack blank:

```sh
nix develop -c cargo run --release --locked -p spice-cad --features rack-tools -- --benchmark
```

The command discards the first paired build and reports six warm samples in
`output/native/benchmark.json`, with geometry and tessellation/audit times
separate. It verifies mesh closure and volume against SMLib's exact solid and
the OpenCascade reference. This planar box-union workload does not establish
performance on curved surfaces or fillets. See `evaluation/native-results.json`
for recorded checks. Run native and reference tests with
`cargo test --workspace --locked`, and lint with
`cargo clippy --workspace --all-targets --locked -- -D warnings` inside
`nix develop`.

### Code-defined models with Luau

Open a typed model in the native editor:

```sh
nix develop -c cargo run --release --locked -p spice-cad -- --model models/mounting-bracket.luau
```

The resizable **Code** pane provides syntax highlighting, line numbers, and text
undo/redo. **Live preview** is enabled by default: committed source edits
rebuild after a 250 ms typing pause, using the existing geometry worker. New
edits replace the pending revision; results and errors from older revisions are
discarded. Errors keep the last valid geometry visible without retrying
unchanged code. For expensive models, disable **Model → Live preview**. **Run**
(Ctrl/Command+Enter) checks and previews the current buffer without saving.
Run is enabled only when source or a pending parameter draft needs building;
it is disabled for a synchronized model and while the worker is busy.
**Save** (Ctrl/Command+S) saves atomically, then checks and rebuilds; a matching
validated preview is reused. Export includes the exact validated source,
including an unsaved draft. Invalid code preserves the last valid geometry and
disables export until the current buffer builds successfully.

External editor saves are detected automatically. If local edits conflict with a
disk change, **Reload disk** discards the local draft; **Keep editor** keeps it
and permits a subsequent save over the disk version. Closing or opening another
model with unsaved edits offers save, discard, or cancel. **File → Open…** (Ctrl+Shift+O) browses `.luau` and `.lua` files without restarting the app;
the path field also accepts an exact file or directory. File loading runs off
the UI thread. The header separates navigation from document actions. **Code**,
**Split** and **Model** switch between editor focus, side-by-side editing and
viewport focus (Ctrl+1 / Ctrl+2 / Ctrl+3). Drag the Code pane boundary to resize
it. **View** controls Properties and Diagnostics. Diagnostics start collapsed;
errors keep the workspace layout unchanged. The footer shows checking progress
or issue counts. Click that indicator or press Ctrl+Shift+J to open the full
list. The footer also shows the current Vim mode, cursor position, and total line count. A viewport
badge distinguishes a current preview from pending changes or errors. Update
model previews unsaved edits; Save writes the file and updates the preview.

**File → New model** (Ctrl+Shift+N) chooses a filename and opens a small parametric
box as an unsaved draft. No file is created until Save; canceling a model switch
preserves the previous source. **Save as** (Ctrl+Shift+S) writes a copy in the
background, switches the active path, and preserves undo history. Existing
targets require confirmation and are checked again before writing. New
destinations are created without overwriting a file that appeared in the
meantime. Both actions, and the Live preview toggle, are available through the
command palette.

Vim-style modal keys are enabled by default and can be toggled in **Edit**.
Escape enters Normal mode; `i`/`a` enter Insert, `v`/`V` select, and `h/j/k/l`,
`w/b/e`, `0/^/$`, `gg/G` move with optional counts. Supported edits include
`d/c/y` with motions, `dd/cc/yy`, `x`, `p/P`, `r` followed by a replacement
character (including Visual and Visual Line selections), `u` and Ctrl+R.
Ctrl+A/X add/subtract a count from the integer under or after the cursor on the
current line. Decimal signs, hexadecimal/binary prefixes, and leading zeroes
are preserved; Visual selections change the first selected number on each line
as one undo step. Ctrl+E/Y scroll by lines and Ctrl+F/B by pages. Ctrl+Y does not
redo. Insert mode supports Ctrl+H/W/U deletion and Ctrl+J/M newline; Ctrl+N/P
request or navigate LSP completions. Ctrl+[ exits to Normal mode.
Unsupported Vim control commands report that they are unsupported instead of
falling through to desktop select-all, cut, or undo. Insert-mode Ctrl+A replay,
Visual block mode, sequential `g` number changes, and jump-list navigation are not implemented. Ctrl+Shift+V
pastes from the system clipboard; `p/P` use the Vim register.
Commands include `:w`, `:q`, `:wq`, `:e [path]` and `:run`. These are native
modal keys rather than embedded Neovim; plugins, macros and full Vim command
compatibility are not provided.

Editor defaults mirror the local Neovim preferences: an 80-column starting width
and guide, relative line numbers with the current absolute number, four-space
indentation, no wrapping, and an eight-line scroll margin. Resizing and long
lines remain unrestricted. Enter carries indentation and adds a level after
common Luau block openings. In Normal mode, Ctrl+D/U make centered half-page
jumps, Ctrl+P or Space P F opens Quick Open, `-` browses the current file's
directory, Space V D or Space X X toggles diagnostics, and Ctrl+K/J moves to the
next/previous located issue. Ctrl+C exits Insert mode. Space Space runs the
current CAD source. These are built-in preferences; the app does not execute
Neovim configuration. See [the Neovim port notes](evaluation/nvim-editor.md) for
scope and remaining features.

**Find** (Ctrl+Shift+F or Normal-mode `/`) searches forward; `?` searches backward.
Standard editor mode also accepts Ctrl+F. Vim mode keeps Ctrl+F for page movement.
Search is literal and case-sensitive, with incremental match counts and visible
highlights. Enter accepts the current result and returns to code; Escape closes
the bar. Normal-mode `n` repeats the search direction and `N` reverses it; both
wrap at the document boundaries. **Replace** (Ctrl+Shift+H; Ctrl+H in Standard mode) offers **Replace match**
and **Replace all**. Replace All runs on a worker and applies as one undo
operation only if the searched revision is still current. Replacements
participate in Live preview; with it disabled, use Run or Save. Regex and
substitution commands are not implemented. Details and limits are in
[native search](evaluation/search.md).

**Quick Open** (Ctrl+P) fuzzy-searches `.luau` and `.lua` files under the
current model's directory, with recent files first. **Commands** (Ctrl+Shift+P)
searches available actions and shows their shortcuts. Both support Up/Down,
Enter and Escape, and are available in the **File** menu. Browse arbitrary paths
with Ctrl+Shift+O. Quick Open uses the same unsaved-change protection as the file
browser. Its background index is bounded to 4,096 files and eight directory
levels; generated and hidden directories and symlinks are skipped. The most
recent 20 files are stored under `$XDG_STATE_HOME/spice-cad/recent-models.json`
(default `~/.local/state/spice-cad/`).

The properties pane opens by default; **View → Properties** toggles it. Settings
menus stay open after toggles; click outside or press Escape to close them.
The pane groups selectable parts, source parameters with full-width sliders,
selected-geometry dimensions, and named measurements. Geometry statistics live
under **Geometry details**.

**Parts** is the model outliner. Its checkboxes hide or show individual parts;
the row menu (`…`) and right-click menu offer **Isolate part**, **Show and frame
part**, and **Reveal source**. Exit isolation restores the previous visibility;
changing a checkbox during isolation keeps the currently visible set and exits
isolation. **Show all** restores every part. F outside the editor frames the
selected part, or the visible assembly when nothing is selected, preserving the
camera orientation. Framing a hidden part shows it first.

Visibility and part selection follow unique `cad.show` names through geometry
rebuilds and output reordering. New names start visible; removed names are
forgotten. Assembly/Print switching keeps visibility. This is per-document
session state and resets when another document is opened. Hidden geometry is
excluded from picking and viewport dimensions. Visibility changes update a
GPU uniform without rebuilding or uploading geometry; exports still include
the entire authored model. Reveal source moves the editor to the output's
`cad.show` declaration, and is disabled until the displayed result matches the
current source revision.

Declare the values you want to expose; ordinary locals remain private:

```lua
local width: number = cad.number(310, { label = "Rack width", min = 100, max = 480, step = 1, unit = "mm" })
local wall: number = cad.number(3, { label = "Wall thickness", unit = "mm" })
local print_layout: boolean = cad.toggle(false, { label = "Print layout" })
```

Numbers with both `min` and `max` get a slider and editable value; other numbers
get a numeric field. Booleans get a checkbox. Labels and units are optional;
`step` defaults to 0.01. Runtime evaluation rejects non-finite values, invalid
ranges, non-positive steps, and values outside declared bounds. Bounds describe
individual controls; geometric constraints between properties still belong in
code.

Only top-level declarations with literal values and literal option tables become
controls. Up to 64 properties are shown from the first 4,096 top-level syntax
nodes; each declaration is limited to 4 KiB. Controls edit only the value
literal, preserving the declaration and comments. With Live preview enabled,
sliders update the model during a drag, at the geometry worker's pace. Only the
latest pending values are kept; the final value is retained after release. Each drag
is one editor undo. Checkboxes commit immediately. Enter or leaving a numeric
field applies it; Escape cancels the pending field edit. Save and Run also commit the active field. Parameter edits bypass the typing debounce; code edits still
wait for a pause. Disable Live preview to edit values and rebuild with Run or Save. Existing personal example files are preserved; the new startup file is
`spice-rack-measurements.luau`, leaving older rack copies intact. See
[navigation and parameter notes](evaluation/navigation.md).

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

Each profile accepts 3–128 vertices with finite coordinates within ±2000 mm,
width/depth at most 1000 mm, and edges at least 0.01 mm long.
Self-intersections, touching boundaries, repeated closing points, and
overlapping edges are rejected. Extrusion height is 0.01–1000 mm; models may
define at most 256 profiles. These profiles use straight segments. Cut holes
with subtract; curved profiles, nested loops and interactive sketch constraints
remain future work.

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
remain mandatory. The scripting environment exposes no file/process access or
module imports. Repeated operand recipes are evaluated again; incremental
geometry caching is not implemented. Interactive sketch editing and per-part
pivots are not implemented. The existing P1S rack
export remains available in the rack-tools build with `--rack`.

Format source with `cargo fmt --all`, Luau files with
`stylua --syntax Luau models native/types`, and Nix with `nixfmt flake.nix`, all
inside `nix develop`. Original Luau integration checks are in
`evaluation/luau-results.json`. Current curved-model and native export checks
are in `evaluation/bracket-results.json`. Embedded editor and viewport checks
are in `evaluation/editor-results.json`. The rope buffer, visible-text rendering
path, and reproducible large-file benchmark are described in
[editor performance](evaluation/editor-performance.md). Background checking,
diagnostic navigation, and resource budgets are described in
[the diagnostics workflow](evaluation/diagnostics.md).

### Measurements and named faces

Open **File → Examples → Measured block**, then click **Measures** in the viewport
toolbar or enable **View → Measurements & face names**. The overlay shows
overall X/Y/Z dimensions, named faces, and declared measurements. Selecting a
face shows its name in Properties; **Copy face selector** copies a Luau
expression for an axis-aligned planar face. Replace `part` with your final solid
variable and choose a unique name.

Dimension styling follows
[Plasticity's Dimension demo](https://doc.plasticity.xyz/common/dimension): thin
extension lines and green values aligned with the projected edges. Labels use
15-point monospace text on opaque dark backgrounds for contrast; hovered or
selected dimensions turn light blue with a blue border. Overall dimensions sit
along the near edges, and a declared measurement matching an overall extent
shares its label. Names and check results appear in Properties and dimension
tooltips; the selected face name appears on the model. Labels are read-only;
edit the Luau source or parameter fields to change the model.

Click any face to show its own dimensions immediately, even with **Measures**
off. A selected planar face shows in-plane spans aligned with its boundary;
rotated rectangles retain their actual side lengths. Curved faces report X/Y/Z
extents, not surface lengths. Small dimensions use extended leaders so an 8 mm
lip remains readable at the rack's overall scale.

Click within 6 screen points of a visible edge to highlight and inspect it.
Straight edges show their length; circular edges and arcs show **R** (radius)
and **Ø** (diameter), both on the model and in Properties. Click inside a face
to return to face inspection. Edge picking follows **View → Appearance → Surface edges**;
Shift-click continues to select faces for separation measurements. These are
approximate reference measurements from evaluated CAD curves. Other curve types
remain selectable but do not report a length or radius.

**Shift-click a second parallel planar face** to measure perpendicular
separation. For lip height, select the lip top and its tread: the reference
measurement is 8 mm in the rack example. The first face is orange and the second
blue. For a pair, Shift-click either face to remove it; click empty space to
clear the selection. Invalid pairs keep the previous selection. Properties shows
the same measurements. Inspection does not edit source or add design checks.

```lua
local height = cad.number(20, { label = "Height", unit = "mm" })
local part = cad.box({ width = 40, depth = 30, height = height })
local top = cad.face(part, { name = "top", normal = "z+" })
local bottom = cad.face(part, { name = "bottom", normal = "z-" })
cad.measure("height", { from = bottom, to = top, min = 2, max = 25, parameter = "height" })
cad.show("Measured block", part)
```

The startup rack exposes **Lip height**, **Usable tread depth**, **Wall thickness**,
and **Joint tip gap**. Click a label or its Properties row to edit its linked
parameter. Usable tread depth equals `jar_diameter + 2 * jar_spacing`; its linked
control edits jar diameter. Joint tip gap includes the current part separation:
with the default 12 mm separation it reads 12.2 mm, and with separation zero it
reads the 0.2 mm joint clearance. These are measurements of the displayed model.
Named measurement selections follow their names through shape rebuilds; removing
a measurement clears that selection. Raw face and edge selections still require
unchanged geometry to preserve their numeric IDs. See
[rack measurement validation and timings](evaluation/rack-measurements.md).

Face references select the final output solid by outward normal (`x+`, `x-`,
`y+`, `y-`, `z+`, `z-`), with optional `at` for the coordinate along that axis.
For repeated faces, add `near = { x = ..., y = ..., z = ... }` in the final
solid's local coordinates. After the normal and optional `at` filters, this
chooses the closest face bounding-box center; ties within 0.001 mm fail as
ambiguous. All three coordinates must be finite and within ±2000 mm.
Selectors must resolve to exactly one planar face. Missing or ambiguous faces
fail the build; face indices are never persistent identifiers. Names and
selectors are reevaluated after every rebuild. Intermediate solids,
oblique/curved face selectors, and propagation of names through later operations
are not supported for measurements. Modeling can select intermediate solids
with the same selectors using `cad.hole`:

```lua
local part = cad.box({ width = 60, depth = 30, height = 8 })
local top = cad.face(part, { name = "mounting face", normal = "z+" })
part = cad.hole(part, { face = top, radius = 2, u = -20, v = 0 })
cad.show("Mount", part)
```

`cad.hole` cuts inward from the selected face. Omit `depth` to cut through the
whole solid along that direction; supply a positive depth for a blind hole.
Offsets `u` and `v` default to zero, measured from the face bounding-box center
in the solid's local coordinates. X-normal faces use +Y/+Z; Y-normal faces use
+X/+Z; Z-normal faces use +X/+Y. These axes are the same for either normal sign.
Resizing the face moves its center and the attached holes. Assembly and print
placements are applied afterward. Centers outside the face or inside existing
openings fail; circles may intersect an edge to form an open cut.

Each handle belongs to its exact input solid. After a cut, select the face on
the new solid before making another hole. Modeling handles do not appear as
measurements on later solids; select final faces again for `cad.measure`.
Hole selection queries planar surfaces and trimmed-face bounds directly from
the B-rep, with a 0.001 mm selector tolerance. Kernel closest-point queries
check centers against the trimmed face, including curved openings. Selection
avoids intermediate tessellation; the cylindrical cutter and boolean result
remain analytic B-rep geometry.
See `models/mounting-bracket.luau` for a four-hole example with two face normals.

`cad.measure` reports perpendicular separation between parallel face planes.
Without `min`/`max` it is a reference dimension; with either limit it becomes an
explicit design check. Failed checks preserve the last valid preview and block
export. Properties distinguishes reference dimensions from passed checks and
marks measurements from an older preview. A code-defined shape is deterministic;
that alone does not prove clearance, fit, or manufacturability. There is no
sketch constraint solver or automatic fully-constrained classification.

`parameter = "height"` explicitly links a measurement to one top-level numeric
`cad.number` control. Unknown, duplicate and nonnumeric declarations fail validation.
Click a named dimension label or its row under **Properties → Measurements** to
select its two faces, reveal the linked control and source literal, and open an
**Edit linked parameter** dialog. The field starts with the parameter's current
value; the measured distance is shown separately. Enter or **Apply** makes one
undoable source edit and schedules the usual preview. Escape, **Cancel**, or a
click outside the dialog leaves the source unchanged. Invalid numbers and
out-of-range values stay in the dialog without changing the model. Apply waits
for the preview worker and current source controls to be ready.

The Properties control is highlighted and scrolled into view. Unlinked
measurements reveal their `cad.measure` source without opening an editor. Opening
an editor requires a preview of the current source revision; an external source
reload cancels any open edit. The bundled `models/measured-block.luau`
demonstrates the link.

A link is an authored association, not proof that the parameter drives the
measurement or equals its displayed value. Formulas may connect them. Dimension
labels remain measured values; the dialog edits the linked parameter directly
without inverting formulas. The Properties control and code remain editable too.
`min`/`max` checks remain independent of this association.

These measurements use the preview mesh, display `≈`, and allow 0.001 mm
tolerance when resolving planes and checking limits. They are bounded preview
checks, not exact kernel metrology. Up to 64 named faces and 64 scripted
measurements are supported.

### Startup and responsiveness

`spice-cad --profile` logs the first UI pass, first observed geometry upload,
and resize passes. Startup markers also separate event-loop/adapter selection,
device and egui initialization, and CAD viewport pipeline creation. These are
CPU-side markers, not GPU presentation timings. The renderer remains wgpu with
automatic backend selection; `WGPU_BACKEND=vulkan` or `WGPU_BACKEND=gl` can
select a backend for controlled comparisons.

See [performance goals and measurements](evaluation/performance.md) for the
measurement method, installation limits, and next priorities. A Linux X11
startup, idle-CPU, memory, and resize harness is in
`evaluation/profile-native.rs`. The [cache audit](evaluation/cache-audit.md)
records ownership, invalidation, heap profiles, and remaining repeated work.
Display preparation reuses the exact volume already calculated during mesh
validation and extracts polygons with a fixed point buffer. See
[display meshing measurements](evaluation/display-meshing.md) for stage timings,
allocation budgets, and the manual profiler command.
**Debug → Performance** and `--profile` expose GPU geometry uploads, buffer
allocations, retained capacity, and uniform uploads, including placement upload
counts and bytes. Moving or rotating finished parts updates small transform/color
records; shape changes upload new geometry and reuse buffer capacity when it fits.
Translations also reuse cached thumbnails, bounds, and face measurements.
During parameter dragging, changed thumbnails wait until the pointer is released
and no parameter has changed for 150 ms. Bounds, face measurements and constraints
still update with every preview; matching images remain cached.
Picking shares canonical CPU geometry and transforms queries through part poses,
avoiding world mesh and sampled-edge copies. With `--profile`, normal exit prints
slider latency median, p95, p99 and maximum over the latest 512 accepted revisions.
Timing runs from control change through GPU prepare, excluding GPU completion and
presentation. Profiling also breaks that interval into dispatch, worker queueing,
build time, result delivery and UI-to-prepare; see the
[preview scheduling investigation](evaluation/preview-latency.md). The native
renderer explicitly requests eframe's one-frame, low-latency vsync configuration.
For an automatic, bounded test without desktop input, run
`spice-cad --profile-preview` for translation or `spice-cad --profile-rotation`
for rotation. Both sweep the bundled rack in memory and close after the final
preview and any pending thumbnails, leaving model files unchanged. The
[native Wayland baseline](evaluation/wayland-preview.md) describes the workload
and why headless display timings can differ substantially from the desktop.
See [rotation preview measurements](evaluation/rotation-preview.md) for the
thumbnail policy and rotation workload.
See [canonical picking measurements](evaluation/canonical-picking.md)
for allocation checks and sustained drag results.
Numeric property edits reuse a successful type check when every other source
byte is unchanged. Runtime assertions, property bounds, and assembly checks still
run; code, metadata, and boolean changes require another type check.

## Browser prototype

`browser/index.html` is a local rack configurator using SMLib WASM 0.3.0. Adjust
dimensions and joint clearance to rebuild all four rack parts and both joint fit
pairs. Inspect the assembled rack, an exploded assembly, or each part's print
orientation with orbit, pan, and zoom controls. Download individual STL and STEP
files or a multi-body assembled STEP. The Rust CLI remains available.

The viewport-first workspace has a collapsible parts browser and dimension
inspector, front/right/top/left/isometric camera controls, and dark, light gray,
or system themes. Open command search with Ctrl+K (Command+K on macOS) or S;
type to filter and press Enter to run the first result. Selecting a part shows
its print orientation; dimension edits always rebuild the complete rack. Rack
and fit-sample downloads are under Manufacturing in the inspector.

Name the project in the top bar. Valid configurations autosave in this browser
and restore on reload. Invalid edits leave the last valid autosave intact. Use
**Save project** (Ctrl/Command+S) to download a portable `.spice-rack.json` file
and **Open project** (Ctrl/Command+O) to reopen one. Imports are checked for the
supported format, dimensions, and printable geometry before replacing your
current work. If browser storage is unavailable, use the downloadable project.

**Print pack ZIP** downloads four rack STLs, four joint-fit STLs, the project
file, a dimensions manifest, and `PRINTING.md` with slicing and assembly steps.
Extract it and print the fit samples first. The pack preserves print
orientations; select your actual printer, nozzle, filament, and plate in Bambu
Studio.

Undo and Redo retain up to 32 validated dimension configurations for the current
tab. Invalid or pending edits can be undone to the last valid configuration; a
new valid edit clears the redo branch. Resetting dimensions is undoable. Use the
toolbar, command search, or Ctrl/Command+Z and Ctrl/Command+Shift+Z outside
input fields (Ctrl+Y also redoes). Input fields keep their native text undo.
Undo history is cleared when the page reloads; the last valid project is
restored from autosave. History does not store model bodies.

Download the two pinned dependencies once from the repository root:

```sh
mkdir -p output/browser/vendor/smlib output/browser/vendor/three
nix-shell -p curl gnutar --run '
  curl -fsSL https://registry.npmjs.org/@classcad/smlibwasm/-/smlibwasm-0.3.0.tgz -o output/browser/smlibwasm-0.3.0.tgz &&
  echo "a0b960e783b3f5e33fe46c584fea828df2356a10595316780574c92ab455f9bc  output/browser/smlibwasm-0.3.0.tgz" | sha256sum --check &&
  tar -xzf output/browser/smlibwasm-0.3.0.tgz -C output/browser/vendor/smlib &&
  curl -fsSL https://registry.npmjs.org/three/-/three-0.170.0.tgz -o output/browser/three-0.170.0.tgz &&
  echo "4a608a355dcaba72e0e5383cdc814303f5b6060b43c238cdf6932dceb699238d  output/browser/three-0.170.0.tgz" | sha256sum --check &&
  tar -xzf output/browser/three-0.170.0.tgz -C output/browser/vendor/three
'
```

Then serve the project and open `http://127.0.0.1:8767/browser/index.html`:

```sh
nix-shell -p miniserve --run 'miniserve . --interfaces 127.0.0.1 --port 8767'
```

Modeling, viewing, and exports run locally in the browser using the cached
assets; no CAD service or npm build step is needed. Downloaded parts use their
print orientations. Print both fit pairs before committing to the full rack.

The browser checks cabinet dimensions, jar height and lifting room, P1S part
sizes, mesh closure/orientation, and the 45° overhang limit before enabling
downloads. Invalid or pending inputs disable exports. Some shorter step
configurations need supports with the current joint layout; the browser rejects
them and names the affected part. Increase the rise or step count, or revise the
joint layout, rather than using an unchecked export.

Independent checks imported all eight STEP parts and the assembled STEP into
OpenCascade for both the default configuration and a 300 mm wide, three-step
configuration with a 70 mm rise, 4 mm walls, and 0.3 mm joint clearance. Both
matched their Rust references with zero extra or missing volume per part.
Assembly positioning, part overlap, and 25/15 jar lifting volumes were checked.
`browser/validation.json` records these checks and the browser interaction
tests. Physical printer fit, hinge clearance, and strength remain unverified.

For optional support geometry experiments, see the
[Support Fins assessment](evaluation/support-fins.md). It records the upstream
workflow, integration constraints, and a proposed print comparison; the fin
engine has not been integrated or physically evaluated on this rack.

## SMLib browser evaluation

`evaluation/smlibwasm.html` recreates the default front-left part with
[`@classcad/smlibwasm`](https://github.com/awv-informatik/smlibpy/tree/main/js)
0.3.0. It includes both dovetail directions, the print orientation, STEP/STL
downloads, two rendered views, and mesh/STEP checks. The production generator
continues to use Rust and OpenCascade.

The October 14, 2026 evaluation passed on Linux with a Ryzen 9 5900X and
Chromium 154. OpenCascade imported the SMLib STEP and measured zero extra or
missing volume against the existing part. Both measured 95 × 174 × 161 mm and
203,856 mm³. Their rendered previews were byte-identical; both STLs had 200
triangles, closed consistently oriented meshes, and no overhangs beyond 45°.

Warm median construction times were **149.5 ms for SMLib WASM** and **317.6 ms
for the equivalent native OpenCascade build in release mode**. Each backend ran
five builds; the first was excluded. Import, export, rendering, compilation, and
process startup were excluded. SMLib's initial module import over local HTTP
took 294.5 ms. These are measurements of one planar part, not a general kernel
benchmark. Curved geometry, advanced fillets, female-joint samples, the complete
assembly, parameter sweeps, and MCP were not evaluated.

`evaluation/results.json` contains the raw measurements, checks, source hashes,
and limitations. Generated exports, preview PNGs, and the captured native
benchmark harness are in `output/evaluation/`. The candidate STEP is 57,078
bytes versus 187,550 bytes for the reference. STL size is not directly
comparable: SMLib emitted binary STL and the reference is ASCII.

To replay the browser checks from the repository root, first generate the
reference files, download the pinned package, and serve the page over HTTP:

```sh
nix develop -c cargo run --locked -p spice-rack
mkdir -p output/evaluation/vendor
nix-shell -p curl --run 'curl -fsSL https://registry.npmjs.org/@classcad/smlibwasm/-/smlibwasm-0.3.0.tgz -o output/evaluation/smlibwasm-0.3.0.tgz'
echo 'a0b960e783b3f5e33fe46c584fea828df2356a10595316780574c92ab455f9bc  output/evaluation/smlibwasm-0.3.0.tgz' | sha256sum --check
nix-shell -p gnutar --run 'tar -xzf output/evaluation/smlibwasm-0.3.0.tgz -C output/evaluation/vendor'
nix-shell -p miniserve --run 'miniserve . --interfaces 127.0.0.1 --port 8767'
```

Open `http://127.0.0.1:8767/evaluation/smlibwasm.html` and select **Run
evaluation**. The browser page uses fixed default dimensions; update it if the
Rust design changes. A browser prototype is supported by these results;
migrating the production generator requires checking the remaining parts and
behaviors.

## Modify the CAD engine

Both the bindings and kernel are available as source. The bindings use LGPL-2.1;
OpenCascade uses LGPL-2.1 with its additional exception. Their license files
govern changes and redistribution.

To work on the Rust bindings, clone them beside this project from the Nix shell:

```sh
git clone https://github.com/bschwind/opencascade-rs ../opencascade-rs
git -C ../opencascade-rs checkout 32758df23a137f4e7c786c618b0317fc3badb3b2
```

Add this section to `Cargo.toml`:

```toml
[patch.crates-io]
opencascade = { path = "../opencascade-rs/crates/opencascade" }
opencascade-sys = { path = "../opencascade-rs/crates/opencascade-sys" }
```

Then run `cargo update -p opencascade -p opencascade-sys`,
`cargo test --locked`, and `cargo run --locked -p spice-rack`. The `builtin`
feature stays disabled, so no OCCT submodule is needed. Commit the changed
lockfile when you intentionally adopt changes in your own version control
workflow.

`flake.lock` pins the tools and kernel; `Cargo.lock` pins Rust dependencies. The
released 0.3.0 bindings require OCCT 7.8 APIs, so the kernel comes from the
Nixpkgs 24.11 input while Rust tools come from the current pinned Nixpkgs input.
To modify the C++ kernel, change the `occt` derivation in `flake.nix` to build
your checkout. Keep kernel and binding API changes coordinated.

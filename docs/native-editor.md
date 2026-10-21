# Native editor

[← Spice CAD](../README.md) · Commands run from the repository root.

## Studio identity

Spice CAD is a [Holy Furries Studio](https://holyfurries.com) tool. The startup
splash uses the approved artwork from [holyfurries/brand-assets](https://github.com/holyfurries/brand-assets),
centered on the studio's forest background across the entire window. The pinned
native bundle contains the original animation baked into PNG atlases, played at
60 fps with decoding off the UI thread and one reused GPU texture. Asset
provenance and update instructions are in
`native/assets/brand/README.md`. Escape, Space, or a click skips the splash.
Startup completion or failure dismisses it immediately and releases its texture;
there is no minimum display time or runtime download. The animation plays once
and holds its final frame if loading continues. Set
`HOLYFURRIES_REDUCED_MOTION=1` to use the approved static poster. The existing
PNG decoder handles both versions. Spice CAD's geometric spice-jar mark remains
in the header and `native/assets/spice-cad.svg`. **Help → About Spice CAD** opens
the product identity card.

## Installation

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

## Viewport controls

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

## Legacy rack tools

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
Tests default to one test thread through `.cargo/config.toml` because native
fixtures share SMLib's global context; this avoids unrelated fixture contention
consuming worker-response timeouts.

## Editing Luau models

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

## Files and workspace

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

## Vim-style editing

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
request or navigate LSP completions. Ctrl+\[ exits to Normal mode.
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
Neovim configuration. See [the Neovim port notes](../evaluation/nvim-editor.md) for
scope and remaining features.

## Search and navigation

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
[native search](../evaluation/search.md).

**Quick Open** (Ctrl+P) fuzzy-searches `.luau` and `.lua` files under the
current model's directory, with recent files first. **Commands** (Ctrl+Shift+P)
searches available actions and shows their shortcuts. Both support Up/Down,
Enter and Escape, and are available in the **File** menu. Browse arbitrary paths
with Ctrl+Shift+O. Quick Open uses the same unsaved-change protection as the file
browser. Its background index is bounded to 4,096 files and eight directory
levels; generated and hidden directories and symlinks are skipped. The most
recent 20 files are stored under `$XDG_STATE_HOME/spice-cad/recent-models.json`
(default `~/.local/state/spice-cad/`).

## Parts and properties

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

## Parameter controls

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
[navigation and parameter notes](../evaluation/navigation.md).

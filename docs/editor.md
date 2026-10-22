# Editor and workspace

[← Spice CAD](../README.md) · [Modeling](modeling.md) · [Printing](printing.md)

## Open and save

`cargo run --release --locked` launches the native application from `nix develop`.
Installed builds run without the checkout. The first launch opens a rounded
cube; **File → Examples** opens other bundled models while preserving
existing personal copies. Each bundled source revision installs into its own
directory, so updated examples use current APIs without replacing older files or
your edits. Module examples are versioned together with their dependencies.

| Action | Shortcut |
| --- | --- |
| New model | Ctrl+Shift+N |
| Browse files | Ctrl+Shift+O |
| Quick Open | Ctrl+P |
| Save / Save as | Ctrl+S / Ctrl+Shift+S |
| Command palette | Ctrl+Shift+P |
| Code / Split / Model | Ctrl+1 / Ctrl+2 / Ctrl+3 |
| Run current source | Ctrl+Enter |
| Diagnostics | Ctrl+Shift+J |

Saving writes atomically. Opening another model or closing with unsaved changes
asks whether to save, discard, or cancel. External changes reload a clean buffer;
conflicts offer **Reload disk** or **Keep editor**. Quick Open searches Luau files
under the active model's directory and lists recent files first.

**Model → Live preview** is enabled by default. Typing rebuilds after a 250 ms
pause; parameter controls submit their latest value as the worker becomes ready.
**Run** builds an unsaved buffer without saving. Invalid code keeps the previous
valid geometry visible and blocks exports until the current revision succeeds.
The preview badge distinguishes stale geometry from unsaved file changes.

## Arrange the workspace

Resize the code pane by dragging its boundary. It starts at approximately 80
columns, with an 80-column guide, relative line numbers, four-space indentation,
no wrapping, and an eight-line cursor margin. Long lines and wider panes work.

**View** toggles Properties, Diagnostics, Build output, measurements, and four
views. Properties opens by default. Diagnostics and Output use full-width,
resizable bottom docks. Messages and Vim command entry do not move the document.
Use **View → Appearance** for surface edges and display options; **Debug →
Performance** exposes profiling counters.

**Results** shows collapsible cards beside executed `cad.show` and `cad.measure`
calls. Click a part card to select and frame it. Old cards are marked as previous
results until a successful rebuild; cards are never written into the source file.

## Navigate and inspect

Drag to orbit, Shift-drag or right-drag to pan, and scroll to zoom. Click a face
or visible edge to inspect it. **F** over the viewport frames the selected part
or visible assembly. **View → Four views** shows Top, Front, Left, and Perspective;
orthographic panes share pan and zoom, while Perspective keeps its own camera.

**Parts** checkboxes control viewport visibility. Right-click a row for isolation,
framing, and source navigation. Visibility follows unique part names through
rebuilds and affects picking; exports still include all authored parts.
**Assembly / Print layout** changes placement without rebuilding the solid.

**Measures** displays overall dimensions and scripted measurements. Selecting a
face shows its dimensions even when the overlay is off. Straight edges report
length; circular edges report radius and diameter. Shift-click a second parallel
planar face to measure separation—for example, a lip top and its tread.
Curved-face spans are bounding extents, not surface lengths.

Named measurements can link to a property. Click their label or Properties row
to reveal that control and edit its value. Labels display measured values; the
editor changes the authored parameter without trying to invert a formula.
See [named faces and checks](modeling.md#named-faces-and-design-checks).

## Vim-style editing

Modal editing is enabled by default; toggle it in **Edit**. This is a native
editor with Vim-style keys. It does not embed Neovim or load its configuration.

| Operation | Keys |
| --- | --- |
| Normal / Insert / Visual / Visual Line | Esc / `i`, `a` / `v` / `V` |
| Move | `h/j/k/l`, `w/b/e`, `0/^/$`, `gg/G`, optional counts |
| Delete / Change / Yank | `d/c/y` plus a motion; `dd/cc/yy` |
| Delete character / Replace / Paste register | `x` / `r` plus a character / `p/P` |
| Undo / Redo | `u` / Ctrl+R, optional counts |
| Increment / Decrement integer | Ctrl+A / Ctrl+X |
| Scroll lines / Pages / Half-pages | Ctrl+E/Y / Ctrl+F/B / Ctrl+D/U |
| System clipboard paste | Ctrl+Shift+V |
| Commands | `:w`, `:q`, `:wq`, `:e [path]`, `:run` |

Insert sessions form one undo block, including newlines and backspaces.
`c` and `o/O` include the following insertion in their undo block. Leaving Insert
mode or moving its cursor begins another block. `r` also replaces Visual selections.
Ctrl+A/X changes the first selected number on each Visual line as one edit.
Ctrl+Y scrolls; it does not redo.

Insert mode supports Ctrl+H/W/U deletion, Ctrl+J/M newline, Ctrl+N/P completions,
and Ctrl+C or Ctrl+\[ to return to Normal. Space Space runs the model; Space P F
opens Quick Open; `-` browses the current directory. Space V D or Space X X toggles
diagnostics, and Ctrl+K/J navigates located issues.

Visual block mode, macros, plugins, jump lists, Insert-mode Ctrl+A replay, and
sequential `g` number changes are unsupported.

## Search and language tools

Ctrl+Shift+F or Normal-mode `/` searches forward; `?` searches backward. Search
is literal and case-sensitive. `n/N` repeats or reverses the direction and wraps
at document boundaries. Ctrl+Shift+H opens Replace; Replace All is one undo step.
Standard editing mode also accepts Ctrl+F/H. Regex substitution is unsupported.

Tree-sitter highlights source in the background. Luau LSP provides completion,
hover, and definitions, including [local modules](modeling.md#local-modules).
Diagnostics link to source locations. Running a model performs a type check
before kernel operations; stale worker results cannot replace a newer revision.

## Application data and branding

Personal examples live under `$XDG_DATA_HOME/spice-cad/models/`, defaulting to
`~/.local/share/spice-cad/models/`. Recent files use
`$XDG_STATE_HOME/spice-cad/recent-models.json`, defaulting to `~/.local/state/`.
Existing example edits are preserved.

The shared Holy Furries splash fills the window and ends when startup completes.
Escape, Space, or a click skips it. Set `HOLYFURRIES_REDUCED_MOTION=1` for its static
poster. Asset provenance lives in
[the brand bundle](../crates/spice-cad/assets/brand/README.md).

# Native navigation and source parameters

## Usage

- Ctrl+P: Quick Open. Fuzzy-match model filenames or relative paths beneath the
  active model's directory; recent files appear first, including files elsewhere.
- Ctrl+Shift+P: command palette. Search actions, inspect their shortcuts, and use
  the same Run, Save, export, view and example-loading paths as the toolbar.
- Up/Down selects a result; Enter accepts; Escape or clicking outside cancels.
- The File menu exposes both palettes and the Ctrl+Shift+O path browser.
- Parameters opens source-derived numeric fields. Enter or blur commits, Escape
  cancels, and Save/Run commit the focused draft before acting on the source.
  Undo in the source editor reverses one complete parameter edit.

## Bounds and ownership

File indexing and recent-file persistence use one lazy background worker, with
one pending job and one completion slot. New requests cancel older scans.
The scan visits at most 16,384 directory entries, keeps at most 4,096 files and
1,024 queued directories, and descends at most eight levels. It does not follow
symlinks or visit hidden/generated directories. Results are cached and fuzzy
ranking runs when the query or index changes; at most 64 results are shown.
Filenames take priority over full-path matches, with recent files first before
typing and as a tiebreaker when searching. Queries have a 256-character
limit; stored paths have a 4,096-byte limit. Recent history holds 20 paths and
uses an atomic rename with a 128 KiB read limit. Filesystem failures appear in
the palette; Browse files remains available when a scan is incomplete.

The worker starts on first Quick Open, rather than at application boot. Dropping
the palette cancels its pending work without waiting on filesystem calls; the
worker exits after any active call returns.

Parameter extraction shares the existing Tree-sitter worker and revisioned rope
snapshot. It reads at most 4,096 top-level nodes and exposes at most 64 single-name
numeric declarations. Decimal and signed decimal literals and exponents are
supported; formulas, nested locals, multiple assignment, hex literals and
non-finite input are excluded. No script runs to discover parameters.
Field commits check the original revision and replace only the literal range,
preserving comments, types and other source. Existing buffer history, diagnostics,
save conflict checks and preview validation still apply.

Opening a file from Quick Open uses the existing loader and unsaved-change
confirmation. Selecting the active file preserves its buffer. A direct load does
not flash the directory browser; read failures reveal that browser with an error.
Frame accounts for viewport aspect ratio, including narrow layouts with the
properties pane open.

## Verification

Tests cover fuzzy scoring and Unicode, recent ordering and bounds, persistence,
skipped directories and symlinks, cancellation, palette keyboard selection and
focus isolation, literal extraction, source preservation, stale revisions,
invalid numbers, one-step undo/redo, and framing at narrow aspect ratios.
Validation logs and GUI artifacts are under `output/native/navigation-*`.

All 58 workspace tests, formatting, all-feature Clippy and the optimized native
build pass. Native GUI checks verify parameter editing, Run through the command
palette, byte-for-byte saved source, one-step undo, Quick Open's unsaved-change
confirmation and cancel path, successful switching, Display settings, empty
results, minimum window size, saving an active field and rejecting non-finite
input without changing the file. Screenshots are in
`output/native/navigation-qa/`; final build/check logs use the
`output/native/navigation-*-final.log` names.

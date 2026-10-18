# Native editor data path and measurements

The native workspace uses a charcoal palette, a resizable code pane, viewport camera controls, a ground grid and orientation axes. Navigation and document actions occupy separate header rows. Code / Split / Model views have Ctrl+1 / Ctrl+2 / Ctrl+3 shortcuts. Display controls live in the top Display menu; geometry and selection are in the optional Properties pane. Timing details are available through Debug → Performance. Diagnostics start collapsed, with progress and issue counts in the footer; Ctrl+Shift+J opens the list. A viewport badge identifies stale geometry independently of file-save state. Save, Run and Export remain separate actions. Open loads models on a file worker and protects unsaved edits.

## Buffer and rendering

`native/src/text_buffer.rs` owns a Ropey B-tree. Inserts and deletes update affected tree paths rather than shifting a contiguous document. Undo retains structurally shared rope roots, bounded to 1,024 edits and 32 MiB of changed text; branching after undo discards redo entries. The budget retains a complete 16 MiB-to-16 MiB replacement as one undo. Undo restores the original revision identity.

The editor shapes only visible rows and columns. Its galley cache retains the current viewport, clears on edits, syntax results or scale changes, and bounds individual line slices to 4,096 characters. A pathological long line does not trigger whole-line layout. Dirty state and preview/export eligibility compare revision IDs in constant time. Background workers flatten their own source snapshots; ordinary editing does not flatten the document on the UI thread. Saving streams rope chunks to an atomic replacement file; external-edit checks remain in place. The watcher checks metadata before reading unchanged files.

Horizontal clipping uses cumulative glyph advances rather than character count.
The syntax worker indexes tab and non-ASCII positions. Visible lines cache prefix
widths; binary searches locate both a prefix and its visible character window.
While syntax is pending, a visible line builds its own temporary index, skipping
plain ASCII rope chunks. That index is reused until the revision changes. Lines
up to 256 characters keep their complete layout to preserve shaping context;
longer lines retain viewport-sized slices. Missing glyphs use the rendered
monospace replacement width. Pointer picking, selection, and cursor reveal share
the same line origins. Width resets after source edits.

Tree-sitter reads rope chunks directly on a separate worker. A single pending snapshot replaces superseded work, and parsing cancels when its revision becomes stale. An edit supplies Tree-sitter byte/point metadata for incremental parsing; undo and missed revisions fall back to a worker-side full parse. Highlight queries cover visible text only, with bounded matches and captures. Until a matching syntax tree arrives, the editor renders plain text. Native modal keys edit the same rope and history rather than maintaining a second editor buffer.

Editing, saving, and execution accept sources up to 16 MiB of UTF-8. The former independent 32 KiB execution cap has been removed. The VM retains its 8 MiB memory limit and 250 ms script budget. Background checking is debounced and runs on a dedicated worker with a single pending rope snapshot; it does not copy the document on the UI thread or rebuild geometry. Analysis has separate time, address-space, and diagnostic-output budgets. Large-file editing does not imply that arbitrarily expensive programs can execute.

## Reproduce

```sh
nix develop -c cargo run --release --locked -p spice-cad --example editor-benchmark
```

The benchmark constructs each rope from an existing string, performs 2,000 distributed single-character insertions, exercises bounded undo/redo, then measures 200 editor UI frames after edits with a 440 × 800 viewport. Five warm-up frames are excluded. These are CPU measurements without disk I/O, GPU drawing, presentation or native-window startup.

A release run on 2026-10-18 after the scrolling fixes measured editor-frame p95
at 0.104 ms for 100,000 lines and 0.144 ms for one million characters on one line.
Insertion p95 was 6.03 µs and 3.09 µs respectively. Results are local CPU timings,
not end-to-end input latency; the log is `output/native/dimensions-editor-benchmark.log`.

One local release run on 2026-10-17, before the Tree-sitter/LSP additions, produced:

| Input | Rope construction | Insert p95 | Editor UI frame p95 |
| --- | ---: | ---: | ---: |
| 108,000 bytes | 0.096 ms | 2.70 µs | 0.062 ms |
| 100,000 lines / 2,400,000 bytes | 0.969 ms | 4.27 µs | 0.060 ms |
| Single line / 1,000,000 characters | 0.347 ms | 1.52 µs | 0.005 ms |

Measurements vary with system load. These historical numbers exclude background syntax and language-server work. The regression tests assert bounded layout work and correct text/history behavior rather than machine-specific latency thresholds. Raw local output is `output/native/editor-benchmark.txt`.

## Validation

The earlier renderer change passed 27 workspace tests and nineteen native X11 checks, including a 100,003-line file edited at its end. Saving after undo restored that large file byte for byte. The close guard runs after editor events so an edit arriving in the close-request frame is considered. Those artifacts remain under `output/native/editor-final-qa/`. The current workspace passes 41 tests, including syntax, LSP, modal and menu-input regressions, recorded in `output/native/editor-tools-tests.log`. Additional GUI artifacts are under `output/native/workspace-qa/`, `output/native/workspace-flows-qa/` and `output/native/file-open-qa/`.

The workspace UI pass also passes 41 tests, both Clippy configurations and formatting. Native checks cover all three view shortcuts, a 1100 × 650 window, collapsed diagnostics, visible error counts, stale-preview feedback, correction and Run, exact-source Save, and the disabled-export explanation. Logs are `output/native/workspace-ui-tests-final.log`, `output/native/uiux-qa-results.log` and `output/native/uiux-preview-results.log`; the final workspace screenshot is `output/native/uiux-preview/showcase.png`.

The subsequent Neovim-preference pass uses an 80-column initial pane width and guide, relative numbers and an eight-line cursor margin. Smart indentation examines bounded 256-character prefix and suffix slices and produces at most 256 spaces; it does not flatten a large line. Native navigation and indentation still edit the same rope. The workspace passes 44 tests, including current-selection diagnostic navigation and bounded indentation. Logs are `output/native/nvim-ui-tests-final.log` and `output/native/nvim-qa-results.log`; the assembled-rack workspace is captured in `output/native/nvim-qa/showcase.png`.

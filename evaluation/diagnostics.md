# Compile and diagnostics workflow

The native app accepts the same 16 MiB UTF-8 source limit for editing, saving and execution. The former 32 KiB execution cap is removed. A 100,000-line comment-heavy model is covered by an integration test that builds a native solid and verifies its volume.

## Background checking

A dedicated worker runs a persistent `luau-lsp` process, started lazily after a 350 ms typing pause. The UI passes a structurally shared rope snapshot; flattening and protocol serialization happen on the worker. There is one pending request and one completion slot. Editing invalidates the desired revision immediately, cancels older requests, and removes old diagnostics. Request sequence IDs also supersede completion or hover requests for the same source revision. Results are published and accepted only for the matching revision.

Language-server requests have a five-second deadline. On Linux the server has a 1 GiB address-space limit. Protocol messages are bounded to 64 MiB, reader queues to 32 messages, diagnostics to 200, and completion items to 100. Definition files expose the existing CAD type context; Roblox platform features and workspace indexing are disabled. Shutdown kills the server before joining workers so a blocked pipe write cannot hold the UI open. A failed server is restarted on the next request.

Explicit Run still invokes the independent `luau-analyze` checker with one thread, a five-second deadline, a 512 MiB address-space limit and a 64 KiB output budget. Its output, timeout and cancellation paths kill and reap the analyzer. Neither checker can override the requirement for a strict source directive.

Typing performs type checking without running scripts or invoking the CAD kernel. Existing explicit Run/Save actions and external-save rebuild behavior remain. Run validates its exact source snapshot independently; it does not trust a potentially stale background result.

## Source navigation

Diagnostics start collapsed. The footer retains checking progress and error/warning counts; clicking the indicator or pressing Ctrl+Shift+J opens the list. Normal-mode Ctrl+K / Ctrl+J follows the next / previous located issue and wraps at the ends. Space V D or Space X X toggles the panel. This keeps issues visible without reserving panel space when the file is healthy.

The resizable Diagnostics panel shows errors and warnings, with clickable line/column links. Clicking reveals the Code pane, scrolls to the range, selects it, and focuses the editor. Language-server positions use UTF-16; Run diagnostics use Luau's byte columns. Both map to rope character indices without splitting Unicode characters. GNU formatter end columns are inclusive; this mapping follows the [pinned Luau reporter](https://github.com/luau-lang/luau/blob/0.740/CLI/src/Analyze.cpp#L58).

Ctrl+Space requests completion, F1 or Normal-mode `K` shows types, and F12 or `gd` follows definitions. Built-in declarations without editable source show type information. Completion edits apply only to their source revision and cursor position. Snippet and additional-edit completions are omitted. Opening another file uses the native file browser and the same save/discard/cancel protection as ordinary file opening.

Runtime chunks use a filename-form name so Luau stack traces provide `model.luau:line` locations. Located runtime failures link to their call site; kernel and operational failures without source locations remain readable, unlocated diagnostics. Build messages are bounded before reaching the UI, retaining both their beginning and stack-trace tail. Editing clears stale run diagnostics and status text.

The existing 8 MiB VM memory budget, 250 ms script budget, 256-operation recipe limit and 512-step geometry evaluation limit remain. Native kernel calls still have no wall-clock cancellation guarantee. Source size alone does not guarantee that a program fits those budgets.

## Validation

Run in the Nix development environment:

```sh
cargo test --workspace --locked --all-features
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo clippy --workspace --locked --all-targets --all-features -- -D warnings
cargo fmt --all --check
```

Tests cover strict mode, model-relative and Unicode ranges, warnings, stale-check replacement and cancellation, large-source native geometry, runtime navigation, bounded error messages, and the existing VM and geometry constraints. Native GUI checks additionally exercise background errors without mesh uploads, an exact token selection at line 100,002 after a Unicode prefix, correction and explicit Run, large-source save, runtime failure with the previous mesh preserved, source navigation, recovery and close confirmation. Local logs and screenshots are under `output/native/diagnostics-qa/` and `output/native/diagnostics-qa-results.log`.

The current workspace passes 44 tests and both Clippy configurations. Language-server, modal, menu-input and diagnostic-navigation tests are recorded in `output/native/nvim-ui-tests-final.log`. Native GUI checks cover resizing, focus, debug metrics, completion and file switching; Cancel preserves the draft, Discard loads the selected model, and Save and open writes the draft before switching. Escape dismisses the browser and `:e` opens a saved model. Earlier logs are in `output/native/workspace-qa-results.log`, with file-switch screenshots under `output/native/file-open-qa/`; preference and example-opening checks are in `output/native/nvim-qa-results.log`. The release binary is built at `target/release/spice-cad`; owned GUI-test processes are stopped after testing.

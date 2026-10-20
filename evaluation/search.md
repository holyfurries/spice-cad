# Native search and replacement

Ctrl+Shift+F opens Find, Ctrl+Shift+H opens Replace, and both are available in the Editor
menu. Normal-mode `/` searches forward, `?` backward, `n` repeats that direction,
and `N` reverses it. Navigation wraps. The bar supports incremental literal,
case-sensitive search, counts, Previous/Next, Replace match and Replace all.
Enter accepts the result without advancing it; Escape returns focus to code.
Closing the bar removes the all-match highlights while preserving the last query.
Queries and replacement strings are limited to 4,096 characters. Regex, case
folding, whole-word options and Vim substitution commands are outside this pass.

## Data path and bounds

`native/src/search.rs` owns a lazy worker with one pending job and one result
slot. Requests carry shared rope snapshots and source revisions. A sequence ID
cancels superseded work; scans check cancellation every 4,096 source characters.
The worker uses a linear KMP scan directly over rope characters, preserving
Unicode and cross-chunk or cross-line matches without flattening the source.
Matches do not overlap, so navigation and replacement use the same index.

Each match uses one `u32` character offset: about 400 KB for 100,000 matches,
bounded to 64 MiB for the pathological 16 MiB document consisting entirely of
one-character matches. Navigation uses binary search. Painting queries only
visible ranges and draws at most 4,096 match rectangles per frame.

Replace All prepares a new rope on the worker. It rejects output above the
shared 16 MiB source limit before constructing it. Applying the result checks the
original revision again, swaps rope roots, and records one history entry.
Concurrent edits cancel old preparation; stale results cannot overwrite them.
Closing the bar cancels preparation that has not yet applied. A replacement that
contains its own search term is skipped when advancing to the next match.

The history accounting budget is now 32 MiB, keeping the 1,024-entry limit. This
retains one undo even when replacing an entire 16 MiB document with another
16 MiB document. Undo restores the original revision identity. Replacements use
the normal dirty-state, diagnostic and preview-invalidation paths. Auto Preview
rebuilds committed replacements after a pause; with it off, Run or Save rebuilds.

## Verification

Workspace tests cover Unicode and rope boundaries, non-overlap, navigation and
wraparound, incremental search focus, stale revisions, cancellation, output-size
rejection, inserted matches, and whole-document undo/redo. Native GUI checks use
the real editor and language-server workflow. All 52 workspace tests pass;
default and all-feature Clippy checks and the optimized native build pass.

The GUI checks verify forward/backward search, Enter focus handoff, `n`/`N`,
no matches, replacement, one-step undo/redo, saving and rebuilding. A separate
100,000-match Unicode fixture verifies indexing, Replace All, byte-for-byte
restoration with one undo, redo and saving. Clipboard assertions wait for the
undo frame before reading the selection. Artifacts and logs are under
`output/native/search-qa/`, `output/native/search-qa-results.log`,
`output/native/search-large-qa/` and `output/native/search-*-final.log`.

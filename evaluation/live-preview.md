# Live preview and model file workflow

Auto Preview starts enabled. A revision-based scheduler observes every committed
buffer edit, including parameters, replacement, undo/redo, completion and disk
reloads. It keeps one pending revision with a 250 ms deadline. The native geometry
worker runs one job at a time; it never accumulates a queue of intermediate edits.
Jobs take shared rope snapshots; flattening runs on the geometry worker, keeping
large source copies off the UI thread. Dispatch happens after the frame's editor
input, so an edit arriving at the deadline can replace the pending request.
An already-running SMLib call finishes normally. Its result or error is accepted
only if its model identity still matches the active document and source revision.
Older results never replace the visible mesh or report errors against new code.

Failed builds keep the last valid preview and disable export for the invalid
source. Unchanged errors do not trigger a retry loop. Undo to an already-visible
valid revision reuses that mesh. Auto Preview off cancels automatic pending work;
Run and Save still request an immediate build, and initial/new models always load.
Typing-pause repaints stop while the worker is busy; worker completion wakes the
UI. No additional preview thread, polling loop or dependency is introduced.

## New Model and Save As

Ctrl+N chooses a `.luau` or `.lua` path before opening an unsaved starter model.
The draft is dirty even before the first edit. Its missing file is expected, and
the watcher does not report it as an error. Existing unsaved-change guards apply
before adopting the draft. Canceling leaves both the old source and destination
untouched. The first Save creates the file without clobbering a concurrent arrival.

Ctrl+Shift+S opens Save As. Destination inspection and writing run on one lazy,
bounded file worker. Existing targets require an explicit Replace action. The
worker checks the inspected content before writing and again before rename.
A temporary file receives the rope chunks and is flushed before commit; creation
uses an atomic hard link, and replacement uses rename. These checks detect changes
before commit, but are not a filesystem-wide lock against noncooperating writers.

After success the document adopts the saved path and revision, recreates its
watcher and language session, and records the recent path. It keeps the current
buffer, cursor and undo history. Edits newer than the saved snapshot remain dirty.
Failures preserve the original document. Cancel is available while choosing or
checking a destination; active writes finish before the application closes.

## Verification

Tests cover debounce and coalescing, manual retries, mode toggling, mesh reuse,
stale build identities, draft creation, conflict protection, and Save As undo and
future-save routing. Logs and native UI artifacts live under `output/native/live-*`.

Validation on 2026-10-18: all 65 workspace tests pass, workspace Clippy with all
features and warnings denied passes, and the release native build succeeds.
Native UI checks cover automatic and manual builds, error recovery with the last
mesh retained, Save As with undo, stale overwrite confirmation, new unsaved drafts
(including clearing and undoing the entire source), first save, and canceled
document switches. A 100,000-line model also passes parameter-driven automatic
preview, undo, window resize, and byte-for-byte save checks.

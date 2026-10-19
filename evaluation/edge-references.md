# Named edge references and selective fillets

`cad.edge(solid, {name, from={x,y,z}, to={x,y,z}})` creates a typed reference to
a complete straight edge on one source recipe operation. Endpoint order does
not matter. `cad.fillet(solid, {radius, edges={reference, ...}})` rounds only the
selected native edges; omitting `edges` retains whole-solid filleting.

## Resolution and safety

Names are unique, contain 1–80 bytes without controls, and are bounded to 64 per
model. Endpoints must be finite, within ±2000 mm, and at least 0.01 mm apart.
Selection lists contain 1–64 distinct handles from the exact solid passed to
the fillet. Unknown options, sparse lists, empty lists, and wrong handle types
fail before geometry evaluation.

Resolution runs on the existing kernel worker against shared boundary edges,
using endpoints and 17 evaluated points to verify straightness within 0.001 mm.
Interior samples are evaluated only after endpoint matching finds a candidate.
Each reference must match exactly one edge; multiple aliases resolving to the
same native edge are rejected. All references resolve before geometry is mutated.
Native pointers stay local to the operation. SMLib's fillet executive receives
only the selected edge array, with circular constant-radius parameters.
Failed builds retain the previous preview and failed exports create no output.

The existing global radius bound remains for whole-solid fillets. Selected
fillets let the kernel determine validity, allowing vertical corner radii larger
than half a plate's thickness. Recipe and evaluation budgets still apply.

## Workflow and limits

Properties offers Copy edge selector for straight edges. The expression contains
literal endpoints; replace these with parameter expressions to follow edits.
Define references after the booleans/transforms producing their source solid,
then pass them to its fillet. Reusing a handle on a later operation is rejected.
This provides deterministic geometric selection, not arbitrary topology tracking.
Circular selectors remain outside this slice. Selective chamfers now reuse this
reference path; see [chamfer notes](chamfers.md).
The embedded Selected edge fillets example demonstrates the workflow.

## Verification

Tests check analytical selective-fillet volume across a width change, preservation
of sharp hole rims and unselected corners, endpoint reversal, copied selectors,
type/source mismatches, duplicate references/aliases, limits, missing matches,
and export failure without files. Native STEP export is independently imported
through OpenCascade and checked for volume, bounds, and closed oriented meshes.
Reentrant Lua option lookups can create CAD objects without holding recipe
borrows; regression checks cover both polygon points and fillet options.

All 88 workspace tests, Clippy with all targets/features and warnings denied,
formatting checks, and the native release build pass. Native UI checks load the
embedded example, inspect an unrounded 60 mm edge, compile the copied clipboard
selector, retain sharp 3 mm hole rims, reject circular selectors without changing
the clipboard, and preserve/recover the preview after a stale endpoint.
Logs and screenshots are under `output/native/edge-reference-*`.

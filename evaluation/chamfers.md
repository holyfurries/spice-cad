# Selective chamfers

`cad.chamfer(solid, {width=2, edges={reference, ...}})` bevels selected straight
edges using the same typed `cad.edge` references and Copy edge selector workflow
as fillets. Omitting `edges` treats all eligible edges. The native Examples menu
and command palette include Selected edge chamfers.

## Dimensions and implementation

Width is the distance across the flat bevel face, matching the pinned kernel's
constant-distance solver. It is not setback along an adjacent face. At a
90-degree corner, setback is width/√2 on each face. At other angles the setback
changes while bevel width stays fixed.

Fillets and chamfers share one bounded recipe representation and native edge
resolution. An explicit Rust enum distinguishes radius from bevel width.
Selected chamfers use SMLib's linear cross-section generator and constant-distance
solver. Their endpoint corners are marked as bevels before native filleting.
Whole-solid chamfers use the kernel's existing chamfer API.

Sizes are finite and within 0.01–1000 mm. The conservative whole-solid size guard
requires a size below half the smallest bounding dimension; selected operations
let the kernel determine validity. Existing limits of 64 named references and
1–64 selected edges apply. Empty, sparse, duplicate, stale, or wrong-source
references fail through the shared validation path. Geometry stays on the worker
and preserves preview/export error handling. No new dependencies are required.

## Verification

Tests verify flat bevel width and analytical removed volume at right-angle and
acute corners, added volume at a concave corner, mixed fillet/chamfer operations,
sharp hole rims, parameter changes, whole-solid chamfers, invalid inputs, and
reentrant option lookups. Native STEP exports are independently imported through
OpenCascade and checked for volume, bounds, and closed oriented meshes.

All 92 workspace tests, Clippy with all targets/features and warnings denied,
formatting checks, and the release build pass. Native UI checks load the embedded
example, inspect a 4 mm bevel and an untouched 60 mm edge, build a chamfer from
the copied selector, preserve 3 mm hole rims, and retain/recover the preview after
an invalid endpoint. Artifacts are under `output/native/chamfer-*`.

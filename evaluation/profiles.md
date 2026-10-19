# Polygon profiles and extrusion

`cad.polygon({{x=0,y=0}, ...})` creates a typed, immutable profile snapshot.
`cad.extrude(profile, {height=6})` creates a solid recipe. Profiles are distinct
from solids, can be reused, and are available in analyzer/LSP declarations.
The native Examples menu and command palette include Extruded bracket.

## Validation and bounds

Profiles contain 3–128 vertices, close implicitly, and accept either winding.
Coordinates are finite and within ±2000 mm; width/depth are at most 1000 mm.
Edges must be at least 0.01 mm and area at least 0.0001 mm². Validation rejects
crossings, nonadjacent touching, adjacent backtracking, duplicate endpoints,
sparse arrays, and unknown point fields. Winding is normalized counterclockwise.
There are at most 256 profiles per recipe, including unused definitions.
Extrusion height uses the existing finite 0.01–1000 mm dimension range.

## Kernel and lifecycle

Profiles stay in the Rust recipe until the existing geometry worker builds the
requested output. SMLib line curves feed its capped linear sweep along +Z from
the XY plane at Z=0. The temporary curve array borrows Rust-owned native curves;
the sweep copies inputs, and all temporaries are released after building the
solid. The array uses the kernel context allocator. Geometry remains native
B-rep throughout booleans, preview tessellation, inspection, and STEP export.
The existing operation/evaluation budgets and last-valid-preview behavior apply.

This slice supports one simple straight-segment loop. Holes use existing solid
subtraction. Curved/nested profiles, sketch constraints, arbitrary extrusion
vectors are outside this slice. Revolve now shares profile construction; see
[revolve notes](revolve.md).

## Verification

Tests check concave volume with circular holes against its analytic value,
closed mesh orientation, clockwise input, collinear vertices, profile reuse,
transforms, immutable point snapshots, invalid shapes, type distinctions, limits,
and LSP completion. The existing export test independently imports the bracket's
STEP through OpenCascade and checks volume, bounds, and mesh closure.

All 83 workspace tests pass, Clippy passes with all targets/features and warnings
denied, and the release build succeeds. Native UI checks load the embedded
example through the command palette, verify its 7337.204 mm³ volume, inspect
the 33.941 mm diagonal edge and 64 mm face span, and clear selection. Logs and
screenshots are under `output/native/profile-*`.

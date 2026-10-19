# Profile revolution

`cad.revolve(profile, {angle=360})` turns a closed polygon section around +Z.
The profile's X coordinate is radius and Y is height, mapped to the XZ plane.
Angles are degrees: positive rotation starts on +X and turns toward +Y.
An empty options table defaults to 360°. Partial turns are capped solids.
The embedded Revolved bushing example has a flange, stem, and through-bore.

## Validation and lifecycle

Existing profile validation and budgets apply: 3–128 points, simple closed
boundary, either winding, and at most 256 profiles and 256 recipe operations.
Revolve adds finite 0.1–360° angles and radius constrained to 0–1000 mm.
Axis contact is allowed; crossing the axis is rejected before kernel work.
Negative profile heights remain valid for preview; export requires Z=0 placement.

Extrusion and revolution share one Rust profile-sweep enum and native curve
construction path. SMLib copies the temporary line curves for its rotational
sweep; all native handles remain on the existing geometry worker. Full turns
close without end caps and partial turns receive caps. Profile definitions are
immutable and reusable, and the result feeds existing booleans/transforms,
inspection, mesh auditing, and export. There are no new dependencies.

## Verification

Tests compare solid/hollow full and partial-turn volumes against analytical
values, check +Z height and quarter-turn orientation, and audit closed meshes.
Cases include cones, frustums, cups, both windings, repeated profile use,
transforms, unions, subtraction, the stepped bushing, invalid angles/radii/types,
and oversized P1S export rejection without files. STEP round trips through
OpenCascade cover a solid turn, the hollow stepped bushing, and a capped sector.

All 96 workspace tests, Clippy with all targets/features and warnings denied,
formatting checks, and the release build pass. Native UI checks load the embedded
bushing, inspect its 5 mm bore radius/10 mm diameter, rebuild it as a half turn
with half the volume, and preserve/recover the preview after an invalid angle.
Logs and screenshots are under `output/native/revolve-*`.

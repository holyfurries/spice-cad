# Named-face hole modeling

`cad.hole(solid, { face, radius, u?, v?, depth? })` cuts from a named,
axis-aligned planar face. The face must belong to that exact input solid.
After each operation, select a new handle on the returned solid. No face IDs
or names propagate across boolean operations.

The local origin is the selected face's bounding-box center. Positive U/V use
Y/Z for X-normal faces, X/Z for Y-normal faces, and X/Y for Z-normal faces,
independent of outward normal sign. The cutter travels inward. Omitted depth
passes through to the far bound of the entire solid; explicit depth creates a
blind cut, or breaks through when it exceeds the available material. A 0.01 mm
external overlap avoids coincident cutter caps without extending blind depth.

Add `counterbore={radius=4,depth=2}` for a cylindrical screw-head seat, or
`countersink={radius=4,angle=90}` for a conical seat. These options are mutually
exclusive. The outer radius must exceed the main hole radius and remain within
the normal 0.01–1000 mm dimension limit. Counterbore depth uses that same limit.
Countersink angle is the included angle in degrees, defaults to 90°, and must
be finite within 1–179°. Its axial depth is `(outer_radius-radius)/tan(angle/2)`
and must not exceed 1000 mm. An explicit blind-hole depth must accommodate the
entire seat; omitted depth preserves the through-hole behavior. A seat may
break through thin material or cross a face boundary. Both remain analytic
cylinder/cone booleans, with the requested outer radius measured at the face.
See `models/manufacturing-panel.luau` for both screw-head styles.

Selection shares the measurement selector's normal, `at`, and nearest-center
`near` rules, with 0.001 mm selector tolerance. Modeling queries the B-rep's
surface planarity and tight trimmed-face bounds without intermediate meshes.
Surface planarity and axis alignment use 1e-8 tolerances. The outward normal
comes from the mate of the material region's face use, matching STEP export
and handling both exterior and internal cavity walls. Face candidates
are limited to 10,000 queried face uses.

The kernel's closest-point query checks the proposed center against the trimmed
face, rejecting distances above 1e-7 mm. This respects circular and concave trims
without approximating them as polygons. A radius may cross a boundary to form
an open cut. Cutter cylinders and boolean output remain analytic SMLib B-rep
geometry. Measurement annotations continue using cached display-mesh planes.

Modeling-only intermediate face handles are excluded from output annotations.
Measurements still require final output faces; a fixed 64-entry index map
preserves measurement references when intermediate handles are skipped.

Cache comparison includes the input geometry, normal/at/near selector, offsets,
radius, depth, and screw-seat type/dimensions/angle. Renaming a reference or changing output placement preserves
the cached geometry. Movement-only cache hits bypass face resolution.

Three local debug runs of `spice-cad --check-model models/mounting-bracket.luau`
before and after replacing intermediate tessellation produced these medians:

| Stage | Tessellated selection (`33a7fd7`) | Direct B-rep selection |
| --- | ---: | ---: |
| Kernel construction | 664.0 ms | 83.2 ms |
| Final display mesh | 307.5 ms | 304.0 ms |
| Total | 993.6 ms | 403.4 ms |

That is approximately 2.5× faster overall and 8× faster kernel construction
for this model on this machine. Volume remained 49560.026 mm³ and final display
geometry remained 2564 triangles. These are small-sample debug measurements,
not release-build or cross-machine guarantees. Final display meshing dominates
the remaining work. A regression test verifies that Rust-side face-query
allocations stay below 4 KiB and unchanged between 5 mm and 100 mm cylinder
radii; kernel-internal C++ allocations are outside that counter.

Validation covers all six normals, blind/through volumes, translated local
coordinates, resizing, assembly placement, final-face measurements, stale and
invalid handles, missing/ambiguous selectors, centers in openings, and cache
reuse/invalidation. The bracket's four removed cylinder volumes remain unchanged.
Additional checks exercise points 0.005 mm inside/outside circular trim
boundaries, nearest-face disambiguation, stepped concave faces, and rejection
of oblique planes, all six internal cavity normals, and double-precision local
coordinates.
Full workspace tests, Clippy with warnings denied, rustfmt, and StyLua passed.
Native headless checks, STEP/STL printing export, GLB game export, and inspection
renders succeeded. Generated artifacts are under `output/direct-face-*`.

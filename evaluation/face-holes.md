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

Selection shares the measurement selector's normal, `at`, and nearest-center
`near` rules. These use tessellated planar bounds with 0.001 mm tolerance.
The hole center must lie in the selected face's triangulation, excluding existing
openings. A radius may cross a boundary to form an open cut. Cutter cylinders
and boolean output remain analytic SMLib B-rep geometry.

Modeling-only intermediate face handles are excluded from output annotations.
Measurements still require final output faces; a fixed 64-entry index map
preserves measurement references when intermediate handles are skipped.

Cache comparison includes the input geometry, normal/at/near selector, offsets,
radius, and depth. Renaming a reference or changing output placement preserves
the cached geometry. Face resolution tessellates the input on a cache miss for
each hole; movement-only cache hits bypass this work. A single local debug run
of the four-hole bracket took 960 ms total versus 461 ms for the previous
explicit-cylinder version. This is a functionality slice, with an observable
rebuild cost; it makes no geometry-speed improvement claim.

Validation covers all six normals, blind/through volumes, translated local
coordinates, resizing, assembly placement, final-face measurements, stale and
invalid handles, missing/ambiguous selectors, centers in openings, and cache
reuse/invalidation. The bracket's four removed cylinder volumes remain unchanged.
Full workspace tests, Clippy with warnings denied, rustfmt, and StyLua passed.
Native headless checks, STEP/STL printing export, GLB game export, and inspection
renders succeeded. Generated artifacts are under `output/face-holes-*`.

# Sweep, loft, and shell

Choose **File → Examples → Sweep, loft and shell**, or open
`models/shape-operations.luau`, for printable examples. These operators
create native SMLib boundary representations and support the existing booleans,
edge treatments, measurements, previews, and STEP/STL/GLB export.

## Sweep

`cad.sweep(profile, {path = {{x=0,y=0,z=0}, {x=0,y=0,z=30}, {x=30,y=0,z=30}}})`

The profile must currently be a polygon. Its bounding-box center follows the
path; the section turns perpendicular to the initial tangent. Paths contain
2–64 three-dimensional points, must be planar and open, and use sharp mitered
corners. Ends are capped. Segments below 0.01 mm, reversals, excessively tight
corners, and nearby nonadjacent segments are rejected. Total length is at most
4000 mm. Curved paths, twisting, scaling, and nonplanar paths are unsupported.

## Loft

`cad.loft({{profile=base,z=0}, {profile=top,z=24,x=2}})`

Supply 2–16 strictly convex polygon sections in parallel XY planes. Each section
has required `profile` and `z`, and optional `x` and `y` offsets. Heights increase
by at least 0.01 mm. All polygons must have the same vertex count and matching
vertex order, with corresponding edges facing the same direction. The kernel
creates a degree-one loft through the sections and caps the first and last.
Profiles with holes, curved boundaries, twist, and smooth higher-degree lofts
are unsupported. Align the first vertex consistently between sections.

## Shell

`cad.shell(body, {thickness=2, open=cad.face(body,{name="top",normal="z+"})})`

Thickness removes material inward while preserving the outer shape. Omit
`open` for an enclosed cavity, or select one named axis-aligned planar face to
create an opening. The face must belong to the supplied source solid. Thickness
must be less than half the smallest bounding-box dimension; curved or narrow
features can impose tighter kernel limits. Shell failures leave the original
recipe unchanged and report an error. Use shells before fillets when possible.

All coordinates are millimetres and remain within ±2000 mm. Complex geometry
can still fail kernel operations; inspect previews and exported geometry before
manufacturing.

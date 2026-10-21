# Sweep, loft, and shell

Choose **File → Examples → Sweep, loft and shell**, or open
`models/shape-operations.luau`, for printable examples. These operators
create native SMLib boundary representations and support the existing booleans,
edge treatments, measurements, previews, and STEP/STL/GLB export.

**File → Examples → Hollow mitered pipe** demonstrates a circular sweep with an
open bore, controlled by radius, wall, and leg-length sliders.
**File → Examples → Curved lofts** includes a cubic circular loft and a rounded
transition. **Smooth hollow sweep** demonstrates an elbow with tangent bends.

## Sweep

`cad.sweep(profile, {path = {{x=0,y=0,z=0}, {x=0,y=0,z=30}, {x=30,y=0,z=30}}})`

The profile can be a polygon or a full `cad.circle({radius=5})`. Its
bounding-box center follows the path; the section turns perpendicular to the
initial tangent. Circle `x`/`y` offsets are recentered onto the path, just like
polygon offsets. Tight-corner and segment-clearance guards use the circle's
actual radius, independent of its source center. Mixed line/arc paths and
nested profile loops are unsupported for sweeping. Paths contain
2–64 three-dimensional points, must be planar and open, and use sharp mitered
corners by default. Ends are capped. Segments below 0.01 mm, reversals, excessively tight
corners, and nearby nonadjacent segments are rejected. Total length is at most
4000 mm. Twisting, scaling, and nonplanar paths are unsupported.

Supply `bend_radius` to replace corners with exact tangent circular arcs:

```lua
local elbow = cad.sweep(cad.circle({ radius = 3 }), {
    path = {{x=0,y=0,z=0}, {x=0,y=0,z=40}, {x=40,y=0,z=40}},
    bend_radius = 12,
})
```

The radius is measured along the path centerline, accepts 0.01–1000 mm, and must
exceed the section's footprint radius (half its bounding-box diagonal for a
polygon). Each corner consumes `bend_radius * tan(turn_angle / 2)` from both
legs; at least 0.01 mm of straight length must remain between bends and ends.
Nonadjacent input segments must be farther apart than the profile diameter plus
twice the bend radius. This conservative guard can reject tightly packed paths.
Nearly straight waypoints that would produce tiny bends should be removed.
Straight and rotational spans preserve analytic surfaces; bends are tangent continuous (G1),
with a curvature change where straight and circular spans meet. There is no
freeform spline-path or nonplanar sweep input yet.

For a hollow pipe, subtract two circle sweeps along the same path. Open
`models/mitered-pipe.luau` for an editable example with exact circular sections
and a sharp elbow. The straight-section volume is π × radius² × path length;
circle boundaries remain exact NURBS until display/export tessellation.

## Loft

`cad.loft({{profile=base,z=0}, {profile=top,z=24,x=2}})`

Supply 2–16 convex single-loop sections in parallel XY planes: polygons,
`cad.circle`, or convex line/arc `cad.path` profiles. Each section
has required `profile` and `z`, and optional `x` and `y` offsets. Heights increase
by at least 0.01 mm. Sections need equal counts of corresponding lines/arcs,
matching order and arc sweep angles, and aligned seam directions. Polygon
sections must remain strictly convex. Concave loops, holes, islands, and
circle-to-polygon transitions are rejected. Align the first vertex or arc seam
consistently between sections. Curves remain exact NURBS boundaries.

The default loft degree is 1. Pass a second argument `{degree = 2}` or
`{degree = 3}` for a smooth quadratic or cubic transition, with at least
`degree + 1` sections. For example:

```lua
local body = cad.loft({
    {profile=cad.circle({radius=12}), z=0},
    {profile=cad.circle({radius=14}), z=12},
    {profile=cad.circle({radius=10}), z=28},
    {profile=cad.circle({radius=6}), z=40},
}, {degree=3})
```

Only the first and last sections are capped. Higher-degree interpolation can
bulge between sections; inspect the resulting bounds and shape before printing.

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

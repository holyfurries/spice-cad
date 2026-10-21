# Measurements and named faces

[← Spice CAD](../README.md) · Commands run from the repository root.

Open **File → Examples → Measured block**, then click **Measures** in the viewport
toolbar or enable **View → Measurements & face names**. The overlay shows
overall X/Y/Z dimensions, named faces, and declared measurements. Selecting a
face shows its name in Properties; **Copy face selector** copies a Luau
expression for an axis-aligned planar face. Replace `part` with your final solid
variable and choose a unique name.

Dimension styling follows
[Plasticity's Dimension demo](https://doc.plasticity.xyz/common/dimension): thin
extension lines and green values aligned with the projected edges. Labels use
15-point monospace text on opaque dark backgrounds for contrast; hovered or
selected dimensions turn light blue with a blue border. Overall dimensions sit
along the near edges, and a declared measurement matching an overall extent
shares its label. Names and check results appear in Properties and dimension
tooltips; the selected face name appears on the model. Labels are read-only;
edit the Luau source or parameter fields to change the model.

Click any face to show its own dimensions immediately, even with **Measures**
off. A selected planar face shows in-plane spans aligned with its boundary;
rotated rectangles retain their actual side lengths. Curved faces report X/Y/Z
extents, not surface lengths. Small dimensions use extended leaders so an 8 mm
lip remains readable at the rack's overall scale.

Click within 6 screen points of a visible edge to highlight and inspect it.
Straight edges show their length; circular edges and arcs show **R** (radius)
and **Ø** (diameter), both on the model and in Properties. Click inside a face
to return to face inspection. Edge picking follows **View → Appearance → Surface edges**;
Shift-click continues to select faces for separation measurements. These are
approximate reference measurements from evaluated CAD curves. Other curve types
remain selectable but do not report a length or radius.

**Shift-click a second parallel planar face** to measure perpendicular
separation. For lip height, select the lip top and its tread: the reference
measurement is 8 mm in the rack example. The first face is orange and the second
blue. For a pair, Shift-click either face to remove it; click empty space to
clear the selection. Invalid pairs keep the previous selection. Properties shows
the same measurements. Inspection does not edit source or add design checks.

```lua
local height = cad.number(20, { label = "Height", unit = "mm" })
local part = cad.box({ width = 40, depth = 30, height = height })
local top = cad.face(part, { name = "top", normal = "z+" })
local bottom = cad.face(part, { name = "bottom", normal = "z-" })
cad.measure("height", { from = bottom, to = top, min = 2, max = 25, parameter = "height" })
cad.show("Measured block", part)
```

The startup rack exposes **Lip height**, **Usable tread depth**, **Wall thickness**,
and **Joint tip gap**. Click a label or its Properties row to edit its linked
parameter. Usable tread depth equals `jar_diameter + 2 * jar_spacing`; its linked
control edits jar diameter. Joint tip gap includes the current part separation:
with the default 12 mm separation it reads 12.2 mm, and with separation zero it
reads the 0.2 mm joint clearance. These are measurements of the displayed model.
Named measurement selections follow their names through shape rebuilds; removing
a measurement clears that selection. Raw face and edge selections still require
unchanged geometry to preserve their numeric IDs. See
[rack measurement validation and timings](../evaluation/rack-measurements.md).

Face references select the final output solid by outward normal (`x+`, `x-`,
`y+`, `y-`, `z+`, `z-`), with optional `at` for the coordinate along that axis.
For repeated faces, add `near = { x = ..., y = ..., z = ... }` in the final
solid's local coordinates. After the normal and optional `at` filters, this
chooses the closest face bounding-box center; ties within 0.001 mm fail as
ambiguous. All three coordinates must be finite and within ±2000 mm.
Selectors must resolve to exactly one planar face. Missing or ambiguous faces
fail the build; face indices are never persistent identifiers. Names and
selectors are reevaluated after every rebuild. Intermediate solids,
oblique/curved face selectors, and propagation of names through later operations
are not supported for measurements. Modeling can select intermediate solids
with the same selectors using `cad.hole`:

```lua
local part = cad.box({ width = 60, depth = 30, height = 8 })
local top = cad.face(part, { name = "mounting face", normal = "z+" })
part = cad.hole(part, { face = top, radius = 2, u = -20, v = 0 })
cad.show("Mount", part)
```

`cad.hole` cuts inward from the selected face. Omit `depth` to cut through the
whole solid along that direction; supply a positive depth for a blind hole.
Offsets `u` and `v` default to zero, measured from the face bounding-box center
in the solid's local coordinates. X-normal faces use +Y/+Z; Y-normal faces use
+X/+Z; Z-normal faces use +X/+Y. These axes are the same for either normal sign.
Resizing the face moves its center and the attached holes. Assembly and print
placements are applied afterward. Centers outside the face or inside existing
openings fail; circles may intersect an edge to form an open cut.

Each handle belongs to its exact input solid. After a cut, select the face on
the new solid before making another hole. Modeling handles do not appear as
measurements on later solids; select final faces again for `cad.measure`.
Hole selection queries planar surfaces and trimmed-face bounds directly from
the B-rep, with a 0.001 mm selector tolerance. Kernel closest-point queries
check centers against the trimmed face, including curved openings. Selection
avoids intermediate tessellation; the cylindrical cutter and boolean result
remain analytic B-rep geometry.
See `models/mounting-bracket.luau` for a four-hole example with two face normals.

Hole options also accept `counterbore = { radius = 4, depth = 2 }` or
`countersink = { radius = 4, angle = 90 }`. The recess radius must exceed the
hole radius; countersink angles are included angles in degrees.
See [recessed holes](../evaluation/face-holes.md) for limits and depth behavior.

`cad.pocket` and `cad.boss` cut inward or add outward from a named face. Supply
`width`/`height` for a rectangle, or a reusable `cad.polygon` as `profile`, plus
`depth` and optional `u`/`v` offsets. Profile coordinates follow the same local
axes as holes. See [face features](../evaluation/face-features.md) and open
`models/manufacturing-panel.luau` for polygon features and both screw-seat types.

`cad.measure` reports perpendicular separation between parallel face planes.
Without `min`/`max` it is a reference dimension; with either limit it becomes an
explicit design check. Failed checks preserve the last valid preview and block
export. Properties distinguishes reference dimensions from passed checks and
marks measurements from an older preview. A code-defined shape is deterministic;
that alone does not prove clearance, fit, or manufacturability. There is no
sketch constraint solver or automatic fully-constrained classification.

`parameter = "height"` explicitly links a measurement to one top-level numeric
`cad.number` control. Unknown, duplicate and nonnumeric declarations fail validation.
Click a named dimension label or its row under **Properties → Measurements** to
select its two faces, reveal the linked control and source literal, and open an
**Edit linked parameter** dialog. The field starts with the parameter's current
value; the measured distance is shown separately. Enter or **Apply** makes one
undoable source edit and schedules the usual preview. Escape, **Cancel**, or a
click outside the dialog leaves the source unchanged. Invalid numbers and
out-of-range values stay in the dialog without changing the model. Apply waits
for the preview worker and current source controls to be ready.

The Properties control is highlighted and scrolled into view. Unlinked
measurements reveal their `cad.measure` source without opening an editor. Opening
an editor requires a preview of the current source revision; an external source
reload cancels any open edit. The bundled `models/measured-block.luau`
demonstrates the link.

A link is an authored association, not proof that the parameter drives the
measurement or equals its displayed value. Formulas may connect them. Dimension
labels remain measured values; the dialog edits the linked parameter directly
without inverting formulas. The Properties control and code remain editable too.
`min`/`max` checks remain independent of this association.

These measurements use the preview mesh, display `≈`, and allow 0.001 mm
tolerance when resolving planes and checking limits. They are bounded preview
checks, not exact kernel metrology. Up to 64 named faces and 64 scripted
measurements are supported.

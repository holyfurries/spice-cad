# Named-face pockets and bosses

`cad.pocket(solid, { face, width, height, depth, u?, v? })` cuts a rectangular
recess inward from a named planar face. `cad.boss` takes the same options and
adds a rectangular support outward. Width follows local U and height follows
local V; depth follows the face normal. All lengths are millimetres.

For a custom footprint, replace `width`/`height` with `profile`, using a reusable
`cad.polygon` handle. For example:

```lua
local outline = cad.polygon({{x=-5,y=-3},{x=6,y=-3},{x=0,y=5}})
part = cad.boss(part, {face=top, profile=outline, depth=4, u=10})
```

Profile X/Y coordinates map directly to local U/V relative to the face origin,
then `u`/`v` offsets apply. Profiles are not recentered or mirrored on negative
normals. Both polygon windings are accepted. Existing polygon rules apply:
3–128 vertices, simple closed boundary without repeated endpoint, no touching
or intersecting edges, width/height at most 1000 mm. Supplying a profile together
with width or height is an error.

U/V axes and origin match `cad.hole`: Y/Z on X-normal faces, X/Z on Y-normal
faces, and X/Y on Z-normal faces, centered on the selected trimmed face's tight
bounding box. Negative normals keep the same U/V directions. Width, height,
and depth must each be finite and between 0.01 and 1000 mm. Offsets default to
zero and must be finite within ±2000 mm.

Select the face from the exact input solid and select again after each
operation. Normal, `at`, and `near` use the existing named-face rules. Missing
or ambiguous faces fail rather than silently choosing another surface.
The center must lie on the trimmed face, outside existing openings. The
footprint may cross its boundary, allowing open pockets and overhanging bosses;
this API does not require full-footprint containment. A pocket deeper than the
available material breaks through. A 0.01 mm overlap avoids coincident boolean
caps while preserving requested blind depth and outward boss height.

Both operations remain exact analytic box or polygon-extrusion B-rep operations. They add no
sketch solver, interactive tools, or automatic face-name propagation. Cache
matching includes feature kind, selector, offsets, dimensions/profile points and input solid;
renaming faces or moving the displayed part reuses geometry.

`models/enclosure.luau` combines a pocket, floor-mounted boss, and blind screw
hole. Run `nix develop -c cargo run --locked -p spice-cad -- --check-model models/enclosure.luau`
to validate it, or open it in the native editor. Its sliders control enclosure
dimensions without changing feature references.

`models/manufacturing-panel.luau` combines an asymmetric L-shaped pocket,
triangular support, counterbore, and countersink. Open it in the native editor,
or run `nix develop -c cargo run --locked -p spice-cad -- --check-model models/manufacturing-panel.luau`.

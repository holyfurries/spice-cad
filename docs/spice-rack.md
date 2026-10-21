# Spice rack

[← Spice CAD](../README.md) · Commands run from the repository root.

The printable model is [`models/rack-parts.luau`](../models/rack-parts.luau).
Open it in the native app, adjust Properties, and export with SMLib. The unsplit
blank in `models/rack.luau` is a construction example. Independent OpenCascade
fixtures remain in test-only verification.

## Cabinet measurements

| Measurement                                   |                                                   Size |
| --------------------------------------------- | -----------------------------------------------------: |
| Cabinet interior width                        |                                                 493 mm |
| Cabinet interior depth                        |                                                 293 mm |
| Cabinet interior height                       |                                                 322 mm |
| Hinge intrusion into the opening              |                                    Approximately 22 mm |
| Hinge height above the cabinet floor          | Approximately 65 mm; measurement reference unconfirmed |
| Current spice rack width                      |                                                 260 mm |
| Width currently remaining for sauces and oils |                                   Approximately 233 mm |
| Target replacement spice section width        |                                   Approximately 310 mm |
| Width remaining beside the replacement        |         Approximately 183 mm, before gaps or a divider |
| Current spice rack levels                     |                                                      3 |
| Current rack arrangement                      |                           Steps rising toward the back |
| Largest spice jar diameter                    |                                    Approximately 50 mm |
| Largest spice jar height, including lid       |                                   Approximately 135 mm |
| Cinnamon-sugar container footprint            |                               Approximately 90 × 90 mm |
| Cinnamon-sugar container height               |                                    Approximately 70 mm |

The replacement should use more of the available height within a spice section
approximately 310 mm wide, leaving the rest for sauces and oils. The prototype
has five rising steps across that width. The square cinnamon-sugar container
stays in the sauces section and is excluded from the rack model. The rack will
be 3D printed on a Bambu Lab P1S; the filament has not yet been specified.

The hinge is a local obstruction. Sliding a low base below it may be possible,
but the taller portions, jars, insertion path, and open door must also clear it.
Its full height and depth footprint still need measuring before modeling a
cutout.

The exported rack is **310 × 283 × 179 mm**, excluding its contents. Its highest
jar platform is 171 mm above the floor. A 135 mm jar reaches 306 mm, leaving 16
mm below the cabinet ceiling. Lifting it 8 mm over its retaining lip leaves 8 mm
of ceiling clearance. The model allows **138 mm jar height**, including that
lift and a minimum 5 mm ceiling gap. Hinge clearance and access around
neighboring jars still need checking.

## Generate the model

The native app opens the four-part rack by default. Choose **File → Export →
3D print · STEP + STL** to export each part in its print placement, or run:

```sh
nix develop -c cargo run --release --locked -- --export-model models/rack-parts.luau output/rack
```

Choose a new directory for each export. It contains `part-01` through `part-04`
STEP/STL files, `parts.json` mapping names to files, the exact Luau source, and
print-check reports. The STEP files describe individual parts in print placement.
For an assembled preview, use Assembly in the viewport or export a game-asset GLB.
Import STLs into the slicer as millimetres and print separate parts.

## Adjust the design

Edit the declarations near the top of `models/rack-parts.luau`, or use the
Properties controls. All lengths are millimetres: X is width, Y is depth from
front to back, and Z is height.

| Parameter | Default | Meaning |
| --- | ---: | --- |
| `width` | 310 | Total rack width |
| `steps` | 5 | Number of rising steps |
| `rise` | 42 | Height difference between steps |
| `jar_diameter` | 50 | Widest jar diameter |
| `jar_spacing` | 1.5 | Jar spacing |
| `wall` | 3 | Platform, side, base, and riser thickness |
| `lip` | 8 | Retaining lip height |
| `joint_clearance` | 0.2 | Dovetail pocket enlargement |
| `separation` | 12 | Displayed part separation; print geometry is unchanged |

Each tread is 56 mm deep, with 53 mm clear behind its front lip. The rack fits
five 50 mm jars per step, for **25 largest-size jars**. Smaller jars may allow
more capacity. Jar geometry is omitted from exports.

The base, stepped side panels, platforms, and risers form a hollow structure
under the raised treads. The rack splits halfway across its width and after the
third step. Integral dovetails join its base, platforms, and outer side panels.
The source declares assembly and bed-oriented print placements separately.

The cabinet/jar clearances above describe the default design. Recheck them when
changing parameters; the current Luau example does not dynamically simulate jar
lifting or hinge interference. The source validates joint dimensions and step
counts, while export checks bed placement, mesh closure, and the printer envelope.
The independent verification fixtures check the default rack's jar and lifting
clearances, joint movement, and print orientations.

## Print and assemble on the P1S

The [P1S build volume](https://bambulab.com/en-us/p1p) is 256 × 256 × 256 mm.
Exports must fit that envelope. Leave room for brims and other slicer additions.
The default parts fit within a 246 mm footprint, with these dimensions:

| Part        | Print dimensions, X × Y × Z |
| ----------- | --------------------------- |
| Front left  | 95 × 174 × 161 mm           |
| Rear left   | 179 × 115 × 161 mm          |
| Front right | 95 × 174 × 155 mm           |
| Rear right  | 179 × 115 × 155 mm          |

1. Print **File → Examples → Fit gauge** with the intended filament for baseline
   clearance calibration. Test a small section of the actual dovetail before
   committing to all four rack parts; the generic fit gauge is not that joint.
1. Adjust `joint_clearance` in Properties or the Luau source if needed. Its
   default is 0.2 mm pocket enlargement. Joints should slide together without
   forcing or cracking; actual fit depends on material and calibration.
1. Import each rack STL into Bambu Studio at 100% scale and center it on the
   plate. Keep the exported orientation, with the broad outer side panel on the
   bed. The default reference fixtures pass a 45° above-bed overhang check.
   Changed dimensions still need slicer review. Check the sliced preview before printing.
   Filament and final wall/infill settings have not yet been selected.
1. Turn the parts upright. For each left/right half, align the front section's
   two side-panel dovetails with the rear section's pockets. Start the rear
   section 3 mm outward from its seated position, then slide it inward across
   the side-panel thickness until the outside faces align. These joints slide
   sideways, rather than pushing front-to-back into the dovetail heads.
1. Lower the completed right half vertically onto the left half, engaging the
   dovetails in the base and raised platforms. All platform tops should be
   flush. Dry-fit the whole rack before any permanent bonding.

The CAD checks do not establish printed load capacity. Check cabinet insertion,
hinge clearance, and platform deflection under the intended jars before use.
Parameter changes must leave the front side panel at least 36 mm tall for its
two dovetails; the generator reports an error otherwise.

## Check changes

```sh
nix develop -c cargo fmt --all
nix develop -c cargo test --locked
nix develop -c cargo clippy --locked --all-targets -- -D warnings
nix develop -c cargo build --locked
nix develop .#verification -c cargo test --locked -p spice-cad-verification
```

Independent verification fixtures check invalid parameters and cabinet collisions, verify closed and
consistently oriented assembled meshes in two configurations, and intersect all
25 jar positions and their vertical lifting volumes against the assembled rack
to prove clear space. They also check every part/sample mesh, print dimensions,
overhang angles, joint clearance, and sampled assembly movements for collisions.

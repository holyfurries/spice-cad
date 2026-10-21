# Spice rack reference

[← Spice CAD](../README.md) · Commands run from the repository root.

This page covers the original rack export CLI. The native app's four-part
example lives in [`models/rack-parts.luau`](../models/rack-parts.luau).

The reference model is written in Rust, using the open-source
[opencascade-rs](https://github.com/bschwind/opencascade-rs) bindings and
[OpenCascade](https://github.com/Open-Cascade-SAS/OCCT) geometry kernel. Model
generation and export run from the command line; no GUI modeling is required.

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

```sh
nix develop
cargo run --locked -p spice-rack
```

The first run downloads the pinned environment and compiles the Rust
dependencies. Later runs reuse the build. Alternatively, run
`nix develop -c cargo run --locked -p spice-rack` without entering a shell.

Exports land in `output/`:

- `spice-rack.step`: assembled CAD geometry for inspection in FreeCAD or other
  CAD software.
- `spice-rack-{front,rear}-{left,right}.stl`: four rack parts, already oriented
  for printing.
- `spice-rack-fit-{width,depth}-{male,female}.stl`: two pairs of small joint fit
  samples.
- Each part and sample also has a STEP file in its print orientation.

STL is unitless; import as millimetres. The generator removes the old
single-piece `spice-rack.stl` after writing the new exports. Print the named
part STLs rather than the assembled preview.

STEP carries the resulting geometry. The parameters and construction logic
remain in the Rust source, rather than a GUI feature history.

## Adjust the design

Edit `PARAMETERS` near the top of `src/lib.rs`, then run
`cargo run --locked -p spice-rack` again. All lengths are millimetres. X is
width, Y is depth from front to back, and Z is height.

| Parameter              | Default | Meaning                                                                  |
| ---------------------- | ------: | ------------------------------------------------------------------------ |
| `rack_width_mm`        |     310 | Total rack width                                                         |
| `step_count`           |       5 | Number of rising steps                                                   |
| `step_rise_mm`         |      42 | Height difference between steps                                          |
| `jar_diameter_mm`      |      50 | Widest jar diameter, including its lid                                   |
| `jar_height_mm`        |     135 | Measured jar height, including its lid                                   |
| `clearance_mm`         |     1.5 | Gap between jars and around jar footprints                               |
| `cabinet_clearance_mm` |       5 | Minimum spare depth and height; also checked against total cabinet width |
| `thickness_mm`         |       3 | Platform, side, base, and riser thickness                                |
| `lip_height_mm`        |       8 | Retaining lip height above each platform                                 |

Each tread is 56 mm deep, with 53 mm clear behind its front lip. The rack fits
five 50 mm jars per step, for **25 largest-size jars**. Smaller jars may allow
more capacity. Jar geometry is omitted from exports.

The base, stepped side panels, platforms, and risers form a hollow structure
under the raised treads. The spice jar height is checked, including room to lift
vertically over the lips. Dimensions exceeding the cabinet allowances cause an
error before export.

The rack is split halfway across its width and after the third step. Its four
parts use integral dovetails in the base, platforms, and outer side panels. The
assembled dimensions and jar capacity stay the same. `src/printing.rs`
constructs the joints, splits the geometry, and positions each part on its outer
side panel for printing. `build_rack` constructs the unsplit geometry.

The model checks finite dimensions, bounded step counts, jar footprints, and the
cabinet envelope before invoking the kernel. The hinge is recorded above but not
modeled because its full footprint is unknown. Change `build_rack` when the
arrangement itself needs to change.

## Print and assemble on the P1S

The [P1S build volume](https://bambulab.com/en-us/p1p) is 256 × 256 × 256 mm.
The generator limits each print footprint to 246 × 246 mm, leaving at least 5 mm
on each side when centered on the plate. With the default dimensions:

| Part        | Print dimensions, X × Y × Z |
| ----------- | --------------------------- |
| Front left  | 95 × 174 × 161 mm           |
| Rear left   | 179 × 115 × 161 mm          |
| Front right | 95 × 174 × 155 mm           |
| Rear right  | 179 × 115 × 155 mm          |

1. Print both male/female fit pairs using the intended filament profile. Keep
   their exported orientations; the width pair tests a joint printed vertically
   and the depth pair tests one printed flat. A brim can help the narrow width
   samples.
1. Dry-fit the samples by sliding across their plate thickness. Adjust
   `JOINT_CLEARANCE_MM` in `src/printing.rs` and regenerate if needed. Its
   default is 0.2 mm enlargement of the pocket profile. The samples should slide
   together without forcing or cracking; actual fit depends on material and
   calibration.
1. Import each rack STL into Bambu Studio at 100% scale and center it on the
   plate. Keep the exported orientation, with the broad outer side panel on the
   bed. The geometry is designed for supports off; tests check all above-bed
   faces against a 45° overhang limit. Check the sliced preview before printing.
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
cargo fmt --all
nix fmt flake.nix
cargo test --workspace --locked
cargo clippy --workspace --locked --all-targets -- -D warnings
```

Tests check invalid parameters and cabinet collisions, verify closed and
consistently oriented assembled meshes in two configurations, and intersect all
25 jar positions and their vertical lifting volumes against the assembled rack
to prove clear space. They also check every part/sample mesh, print dimensions,
overhang angles, joint clearance, and sampled assembly movements for collisions.

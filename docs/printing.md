# Printing and fit

[← Spice CAD](../README.md) · [Modeling](modeling.md) · [Releases](releases.md)

## Export separate parts

Choose **File → Export → 3D print · STEP + STL**, or run from `nix develop`:

```sh
cargo run --release --locked -- --export-model examples/rack-parts.luau output/rack
```

Choose a new output directory. Multipart exports contain numbered STEP/STL files,
`parts.json` mapping filenames to authored names, the exact source graph, and
`PRINT_CHECKS.json` / `PRINT_CHECKS.md`. Exports use declared print placements,
regardless of the displayed layout. Without print placements they use assembly
placements. See [placements](modeling.md#part-placements-and-assemblies).

Import STLs as millimetres at 100% scale, preserving the exported orientation.
Slice separate parts individually. The P1S envelope is 256 × 256 × 256 mm;
leave space for brims and other slicer additions. Parts must meet envelope and
Z=0 bed-placement checks. STL uses a 0.02 mm chord tolerance and a closed-mesh
audit. Game exports have separate tessellation and placement rules.

## Print checks

Open **Model → Print checks** after a successful build, then choose **Run print
checks**. Opening the window does not run them. Checks use final cached geometry
on the worker. Editing marks the report stale until it is rerun. Select a finding
to frame the affected part or pair and show its sampled endpoints.

```lua
cad.print_options({minimum_wall=0.8, minimum_clearance=0.2})
```

Declare this at most once per model. Wall thresholds accept 0.01–10 mm; clearance
accepts 0–5 mm. Those are also the default 0.8/0.2 mm warning thresholds. Expose
thresholds with `cad.number` if they should appear in Properties.

| Check | Coordinates | Effect |
| --- | --- | --- |
| Printer envelope and bed contact | Print placement | Blocks invalid print exports |
| Sampled thin walls | Print placement | Advisory |
| Sampled close surfaces | Assembly placement | Advisory |
| Exact solid overlap | Assembly placement | Advisory; failed pairs marked incomplete |

Collision checks filter part bounds, then use native B-rep intersections and
volume integration. Shared volumes above `0.000001 mm³` produce findings;
zero-volume face/edge/point contact is allowed. Numerical kernel tolerances apply.
Results cover up to 32 parts and cache unchanged solid/placement pairs. Setting
minimum clearance to zero disables sampled gaps but retains collision checks.
Intentional interference fits can still export as separate printable parts.

Wall checks sample at most 128 triangle centroids per part. Surface-gap sampling
has a bounded triangle-visit budget; unexamined pairs are reported. Samples can
miss small features, and a sampled gap is an upper bound on the actual minimum.
Missing warnings do not prove printability or strength. Per-part envelope checks
do not validate packing every part onto one plate.

For JSON suitable for tooling:

```sh
cargo run --release --locked -- --check-print examples/fit-gauge.luau
```

Invalid size or bed placement produces a report and a failing exit status.
Print checks run on demand or during export, not on every slider preview.

## Calibrate fit

**File → Examples → Fit gauge** supplies five pin/receiver pairs with increasing
per-side clearance. A 0.1 mm per-side gap adds 0.2 mm to the opening's total width
and depth. Export them and use the same filament, orientation, and settings as
the final part. Part names record the gap; `parts.json` identifies their files.
These samples have not been physically validated. A generic gauge does not
replace a small sample of the actual joint.

## Spice rack for the P1S

The startup model is [the four-part rack](../examples/rack-parts.luau).
[The unsplit rack](../examples/rack.luau) remains a construction example and
exceeds the P1S bed. Choose **File → Examples → Spice rack · four parts** to open
a personal copy. The default design holds 25 jars of the largest measured size.

| Source parameter | Default, mm unless stated | Meaning |
| --- | ---: | --- |
| `width` | 310 | Total width |
| `steps` | 5 steps | Rising platforms |
| `rise` | 42 | Height between platforms |
| `jar_diameter` | 50 | Largest jar diameter |
| `jar_spacing` | 1.5 | Gap between jars |
| `wall` | 3 | Base, sides, risers, and platforms |
| `lip` | 8 | Retaining lip height |
| `joint_clearance` | 0.2 | Dovetail pocket enlargement |
| `separation` | 12 | Display separation; print shape is unchanged |

Use Properties or edit the declarations in the model. Each tread is 56 mm deep,
with 53 mm clear behind the lip. The assembly is 310 × 283 × 179 mm, excluding
jars. Its hollow structure splits halfway across its width and after the third
step, with integral dovetails in the base, platforms, and outside side panels.
Set separation to zero to inspect the seated assembly. The named joint-tip gap
includes display separation: it reads 12.2 mm at default separation and 0.2 mm
when seated.

| Part | Exported print dimensions, mm |
| --- | --- |
| Front left | 95 × 174 × 161 |
| Rear left | 179 × 115 × 161 |
| Front right | 95 × 174 × 155 |
| Rear right | 179 × 115 × 155 |

1. Print a small sample of the actual dovetail with the intended filament.
   Adjust `joint_clearance` until it slides without forcing or cracking.
2. Import each STL separately into Bambu Studio. Keep its broad outer side on
   the bed and review sliced layers. Default references pass a 45° above-bed
   overhang check; changed dimensions still need slicer review. Filament and
   final wall/infill settings have not been selected.
3. Turn the pieces upright. For each left/right half, align the two side-panel
   dovetails between front and rear sections. Start the rear piece 3 mm outward
   from its seated position, then slide it inward across the side-panel thickness
   until outside faces align. These joints slide sideways, not front-to-back.
4. Lower the completed right half vertically onto the left, engaging dovetails
   in the base and raised platforms. Check flush tops and dry-fit everything
   before permanent bonding. The front side panel must remain at least 36 mm
   tall for its two dovetails.

### Cabinet measurements and remaining checks

These are the original design inputs, not measured manufacturing tolerances:

| Measurement | Size |
| --- | --- |
| Cabinet inside width / depth / height | 493 / 293 / 322 mm |
| Hinge intrusion into opening | Approximately 22 mm |
| Hinge height above floor | Approximately 65 mm; reference unconfirmed |
| Existing three-level rack width | 260 mm |
| Space beside existing / replacement rack | Approximately 233 / 183 mm, before gaps |
| Largest jar diameter / height including lid | Approximately 50 / 135 mm |
| Cinnamon-sugar container footprint / height | Approximately 90 × 90 / 70 mm |

Steps rise toward the back. The cinnamon-sugar container stays beside the rack
with sauces and oils. The top platform is 171 mm above the floor; a 135 mm jar
reaches 306 mm, leaving 16 mm below the ceiling. Lifting it 8 mm over the lip
leaves 8 mm. The default design permits a 138 mm jar with a 5 mm ceiling gap.

The current Luau example validates joint dimensions and step count; it does not
dynamically simulate jar lifting or hinges. Independent reference tests check
default jar/lifting clearance, joints, and print orientations. Remeasure the
hinge's full footprint and verify cabinet insertion, door access, neighboring
jars, and platform deflection before use. Printed load capacity and physical fit
remain unverified.

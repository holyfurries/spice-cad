# Print preparation

Declare advisory thresholds in the model; use `cad.number` to expose them as
Properties controls:

```lua
local minimum_wall: number = cad.number(0.8, {
    label = "Minimum wall warning", min = 0.4, max = 2, unit = "mm",
})
cad.print_options({ minimum_wall = minimum_wall, minimum_clearance = 0.2 })
```

`cad.print_options` may appear once per model. Wall thresholds accept 0.01–10 mm;
clearance thresholds accept 0–5 mm. Zero clearance disables gap warnings. Defaults
are 0.8 mm wall and 0.2 mm clearance. These are configurable checks, not printer
or material guarantees.

## Check a model

Open **Model → Print checks** (also under **View** or in the command palette),
then choose **Run print checks** after the current model finishes building.
The check runs in the geometry worker using cached final geometry. Editing and
camera movement stay responsive; opening the window does not run checks.

The window lists P1S envelope/bed blockers, sampled thin walls, close assembly
surfaces, and exact solid overlaps. Thresholds come from `cad.print_options` in the source. Select a row
to reveal and frame its part or pair of parts. Wall samples use print placement;
gap samples switch to assembly placement. The viewport highlights the affected
parts and shows the sampled endpoints with a readable millimetre callout, including
zero gaps. Envelope and placement blockers show the part's bounding dimensions.
Use **Clear highlight**, click in the viewport, or close the window to clear the
finding. Editing the source, parameters, or imported modules marks the report
out of date and disables its findings until checks are run again.

Assembly collision checks use native B-rep intersections and volume integration,
after bounding-box filtering. They detect containment as well as partial overlaps.
Overlap volumes above `0.000001 mm³` produce advisory findings; touching faces,
edges, and points with no shared volume are allowed. Numerical kernel tolerances
apply. A failed pair is explicitly marked incomplete. Select an overlap to frame
and highlight both parts in assembly placement. Intentional interference fits
remain printable as separate parts; collisions do not block those exports.

Checks cover every pair of the model's maximum 32 parts. Pair results are cached
by canonical solids and assembly placements. Unchanged pairs reuse their results;
print placement, color, and label changes do not invalidate collision geometry.
Checks are on demand and do not run during slider previews or camera movement.
Setting minimum clearance to zero disables sampled gaps, but keeps collision checks.

For a machine-readable report:

```sh
nix develop -c cargo run --release --locked -- --check-print models/fit-gauge.luau
```

The JSON report includes native bounding-box checks for the P1S's 256 mm envelope
and Z=0 bed placement, sampled wall thicknesses, and close assembly surfaces.
Invalid bed placement or size makes the command fail after printing its report.
Thin-wall and clearance findings are advisory. Normal print exports also include
`PRINT_CHECKS.json` and `PRINT_CHECKS.md` with the actual source snapshot.
JSON sample endpoints identify their coordinate space: walls in print placement,
gaps in assembly placement.
`assembly_collisions` records pair counts, executed intersections, cache hits,
overlap volumes, and incomplete pairs, all in assembly placement.

Wall checks cast inward from at most 128 triangle centroids per part. Clearance
checks sample centroid-to-triangle surface distances between nearby assembly
parts. Each pass has an eight-million triangle-visit budget; unexamined pairs
are reported. Samples can miss small features and intersections. A sampled gap
is an upper bound on the actual minimum, and is not a collision certificate.
Curved surfaces depend on tessellation. Missing warnings do not prove a model is
printable. The checks run only when explicitly requested or exporting; slider
previews and camera movement do not pay this cost.

Envelope checks apply to each part's print placement. Gap checks use assembly
placements so intentional mating contact can be reviewed. This does not validate
arranging all exported parts together on one plate.

## Calibrate fit

Open **File → Examples → Fit gauge**. Five receiver/pin pairs provide increasing
per-side clearance, exposed through sliders. A 0.1 mm per-side gap adds 0.2 mm
to the opening's total width and depth. Assembly view shows the mating pieces;
Print layout separates them. Export STEP/STL, preserve the numbered part names
in the slicer, and print with the same filament, orientation, and settings as
the final part. The receiver names record their gap; `parts.json` maps names to
filenames. Start with a small batch and choose the gap that gives the intended
slip or press fit. These samples have not been physically validated.

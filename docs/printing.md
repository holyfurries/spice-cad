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

```sh
nix develop -c cargo run --release --locked -- --check-print models/fit-gauge.luau
```

The JSON report includes native bounding-box checks for the P1S's 256 mm envelope
and Z=0 bed placement, sampled wall thicknesses, and close assembly surfaces.
Invalid bed placement or size makes the command fail after printing its report.
Thin-wall and clearance findings are advisory. Normal print exports also include
`PRINT_CHECKS.json` and `PRINT_CHECKS.md` with the actual source snapshot.

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

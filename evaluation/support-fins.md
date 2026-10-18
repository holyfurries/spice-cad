# Support Fins assessment

Reviewed 2026-10-16. This is a source/documentation review, without a mesh-engine benchmark or physical print trial.

## What it provides

[Support Fins](https://github.com/gittrahan/support-fins) is an MIT-licensed browser tool that imports STL, STEP,
and 3MF, then exports meshes containing removable support geometry. Its workflow lets the user choose an
orientation and load direction. It also offers sway braces for tall parts. The upstream strength and print
claims have not been verified on this rack.

## Fit for this repository

Our four default rack parts already pass the support-free geometry checks in their exported orientations.
Adding fins does not establish a benefit for that configuration. Possible experiments are stabilizing a
tall part or testing a different orientation for layer strength. A changed orientation and its supports
must still fit the P1S plate and height limits.

The [fin specification](https://github.com/gittrahan/support-fins/blob/main/docs/FIN-SPEC.md) describes contact
tines that join the support to the part; their height must match the slicer's layer height. That makes
nozzle, layer height, material, removal marks, and actual joint fit part of the evaluation. Contacts should
avoid our dovetails and mating surfaces.

## Integration decision

Keep this optional for now. The [engine entry point](https://github.com/gittrahan/support-fins/blob/main/web/fins.js)
coordinates several geometry modules, including mutable nozzle and support settings. It is not a single
isolated function to copy into our viewer.

For a first trial, export one rack STL, open it in [the upstream app](https://printfins.com), set the actual
nozzle and layer height, and compare the exported result in Bambu Studio. Check first-layer contact,
support coverage, build bounds, time/material, removal, and joint fit against the original print.

If that trial demonstrates a benefit, a browser integration should pin the upstream revision and license,
retain the original part separately, and validate the combined supported export. Our current overhang gate
must remain intact; adding fins requires a separate coverage check rather than disabling that gate.

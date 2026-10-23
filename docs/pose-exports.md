# Named poses and game exports

Luau remains the editable source; game delivery uses glTF 2.0/GLB. Meshes,
hierarchy, transforms, materials and rigid animation clips use standard fields.
Regular import does not require companion JSON.

## Author and export poses

Declare poses before parameters. Stable IDs are independent of variable names
and UI labels:

```lua
cad.pose("idle", { stride = 0 })
cad.pose("step-left", { stride = 1 })
cad.pose("step-right", { stride = -1 })
local stride = cad.number(0, {
    id = "stride", label = "Stride", min = -1, max = 1, step = 1,
})
```

Normal editing uses literal defaults. Batch export applies overrides without
rewriting source. IDs must be unique across the model and imported modules.
IDs and pose names contain 1–80 ASCII letters, digits, underscores or hyphens.
Limits are 32 poses and 64 parameters per pose. Unknown IDs, incorrect types and
out-of-range selected values fail validation.

Choose **File → Export → Named poses · GLB**, or use a new destination:

```sh
nix develop -c cargo run --locked -- --export-game-poses examples/infantry.luau output/infantry-poses
```

One process shares analysis, operation and tessellation caches. Placement-only
poses reuse geometry; changed shapes rebuild affected parts. Each pose directory
contains an ordinary static GLB, unchanged source bundle and verification metadata
recording its selected pose. Reproduce a snapshot by exporting its named batch.

When geometry, materials and hierarchy match, the batch root also contains a
standard `model.glb` library sharing one mesh set. Named animations are single-key
STEP clips assigning complete node TRS, so switching poses cannot retain another
pose's transform. These are static pose clips, not timed walk cycles. The default
scene is the first declared pose. Geometry-changing alternatives remain separate
GLBs; the manifest explains why no shared library was generated.

`batch.json` records source identity, parameters, GLB hashes, cache hits and
build/tessellation timings. It is written last; a batch without it is incomplete.
Existing export directories are refused.

## Named reference bounds

Select meaningful geometry instead of fitting the entire equipped character:

```lua
cad.reference("body", {
    parts = { "Body", "Head", "Helmet", "Left boot", "Right boot" },
})
```

Missing or ambiguous parts are export errors. Use conditional membership for
optional equipment. Limits are 16 references with 1–32 distinct parts each.

Bounds use actual tessellated vertices after every hierarchy, model and export
transform. GLB `extras.spice_cad.references` contains named `minimum`, `maximum`,
`size`, node indices, `units: "metres"` and `space: "exported world"`. Library
references describe its default pose, not an animation-wide swept bound.
Attachment roles and stable node IDs also live in node `extras.spice_cad`.
These are documented application semantics in a standard custom-property
container, not a ratified measurement extension.

The game chooses its reference and target size. The infantry adapter uses the
idle body's Y extent for every pose; spear movement never changes body scale.

## Provenance and verification

Identity, creator, source and license use optional standard `KHR_xmp_json_ld`
metadata. Ignoring it does not change rendering. Required material extensions
retain their semantics.

`asset.json` is a hash-bound source/build and verification manifest. Its derived
references repeat embedded values for checking. The strict Bevy gate compares
references with loaded vertices and checks animation counts. Loading does not
imply art approval.

References: [glTF specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html),
[XMP extension](https://github.com/KhronosGroup/glTF/blob/main/extensions/2.0/Khronos/KHR_xmp_json_ld/README.md).

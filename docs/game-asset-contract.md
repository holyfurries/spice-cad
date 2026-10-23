# Game asset contract

Game exports contain `model.glb`, `asset.json`, `GAME_ASSET.md`, and a complete
editable Luau source bundle. `asset.json` schema version 1 describes that exact
GLB, including its final world bounds. It does not certify target-engine loading
or art approval.

## Authored identity and provenance

Declare metadata once, in the entry model or an imported module:

```lua
cad.asset({
    id = "crowns-adrift/farmstead",
    author = "Holy Furries Studio",
    source = "examples/farmstead.luau",
    license = "MIT",
    approval = "trial",
    display_width = 1.15,
})
```

`display_width` is the intended X extent in exported world metres, not gameplay
tile size. It records intent; it does not transform geometry. Approval is an
authored declaration: `unreviewed`, `trial`, or `approved`. Omitted provenance
stays null. Without an authored ID, identity derives from sorted semantic
hierarchy paths; renaming or reparenting changes that generated identity.

Parts and frames receive IDs derived from their named hierarchy paths,
independent of dimensions and export transforms. Materials receive IDs from
their names and semantic users. Changing a material's assignment can change its
ID. Repeating an unchanged export preserves geometry ordering and semantic IDs.

Mark attachment frames explicitly; these are rigid transforms, not bones:

```lua
local grip = cad.group("Weapon grip", {
    attachment = "weapon_grip",
    assembly = { position = { z = 30 }, rotation = { y = 90 } },
})
```

## Coordinates and display scale

`coordinates` records source millimetres, the `(x,z,-y)/1000` conversion, exported
right-handed metres with +Y up/+Z forward, model rotation, selected source axes,
and actual export-root TRS. Matrices are column-major. Selected forward/up refer
to assembled CAD axes after model rotation. All recorded transforms are already
in the GLB. Retain its hierarchy; do not apply those transforms twice.

`bounds` contains exact vertex bounds after all hierarchy/root transforms and a
ground-plane anchor at `(0, minimum_y, 0)`. The anchor need not lie on a surface.
`display.additional_scale` describes the optional extra uniform scale to reach
an authored display width. With no display width it is 1, with no inferred art
size. Gameplay tile size remains null.

The farmstead's default miniature export needs display scale 10 for the tested
1.15-unit roof width. To include that scale in the GLB instead:

```lua
cad.model_options({ game = { preset = "crowns_adrift", scale = 10 } })
```

That export needs an identity import wrapper. The preset itself never guesses
asset dimensions or display scale.

## Integrity and compatibility

`files` inventories every bundled source with SHA-256 hashes, original input
hashes before import-path rewriting, a source inventory hash, and the exact GLB
hash. Sources come from the captured preview/export snapshot, not rereads of
possibly changed modules. `exporter` records version, build-time revision and
tracked dirty state when Git is available, plus a build-time SHA-256 fingerprint
of application sources, API types, assets, dependency locks and build settings.
The exporter does not read or hash its executable at runtime.
A build without Git reports unavailable revision/dirty values as null.

Counts distinguish triangles from GPU vertices split by position/normal.
Feature fields explicitly report absent UVs, textures, skeletons and animation.
The material profile preserves the supported OpenPBR subset and declares required
glTF extensions. Zero-specular materials require `KHR_materials_ior` and
`KHR_materials_specular`; unpatched gltf-json 1.4.1 rejects the latter during
strict validation. The export menu and generated handoff warn about that loader
constraint. Do not remove required extensions or disable validation to hide it.

`compatibility.target_loader_validation` is `not_run`: saving a GLB is not a
loader acceptance result. Keep independent loader/render validation reports
alongside the bundle and bind them to its GLB SHA-256.

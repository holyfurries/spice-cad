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

`display_width` is an optional author suggestion for the X extent in exported
world metres. It does not transform geometry or set gameplay sizing policy. Approval is an
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

Crowns Adrift now derives its wrapper scale from its own target width divided
by `bounds.size[0]`; it ignores display suggestions. The preset above happens
to derive scale 1. Neutral, unit-correct exports are equally valid. For troops,
choose a body/reference extent rather than fitting weapons into a universal
box. The preset itself never guesses asset dimensions or display scale.

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

## Strict target-loader gate

The independent `tools/game-import` package pins Bevy 0.19.1 and corrects its
validator through a vendored gltf-json 1.4.1. It runs without a GPU or renderer
and is excluded from the application's Cargo workspace and normal tests.

```sh
nix develop -c cargo build --locked -p spice-cad
nix develop -c cargo test --locked --manifest-path tools/game-import/Cargo.toml
nix develop -c cargo test --locked --manifest-path tools/game-import/Cargo.toml --test bundles -- --ignored --nocapture
```

The last command checks thirteen cases: farmstead, asymmetric facing marker,
and a nested grip/equipment hierarchy, each with default, preset, and rotated
settings, plus three infantry poses and their shared animation library. It
checks source/GLB hashes, strict loading, actual loaded colors,
roughness, metalness, IOR and reflectance, hierarchy/local transforms, counts,
and final bounds, named reference measurements, animation counts and pose-cache
reuse. See [named pose exports](pose-exports.md) for the authoring API and embedded
standard/custom metadata boundary. Each subprocess has a 120-second limit. Bundles, logs and
SHA-bound reports remain under `output/game-import-acceptance/`; set
`SPICE_IMPORT_ARTIFACTS` or `SPICE_CAD_BIN` to override artifact/executable paths.
Tagged-release CI runs this gate against the packaged application and retains
evidence. Ordinary CI does not compile Bevy.

To validate a separate export:

```sh
nix develop -c cargo run --locked --manifest-path tools/game-import/Cargo.toml -- output/farmstead output/farmstead-loader.json
```

The report path must be new. This proves import fidelity, not rendered appearance.
Use the existing Crowns Adrift baker for camera, lighting and native-size checks.

For the consumer correction, copy `tools/game-import/vendor/gltf-json` into the
consumer repository, add a workspace `[patch.crates-io]` entry pointing to that
copy, and enable its `allow_external_materials_specular` feature through a direct
`gltf-json = { version = "=1.4.1", features = ["allow_external_materials_specular"] }`
dependency in the importer. Remove the trial's `settings.validate = false`.
Do not enable the upstream typed `KHR_materials_specular` feature for this Bevy
version: it consumes the raw extension data that Bevy's parser expects. The gate
explicitly tests that the raw zero-specular value survives, while unknown required
extensions and malformed documents still fail. This correction is specific to
Bevy's supported extension mapping; it does not claim other consumers support it.

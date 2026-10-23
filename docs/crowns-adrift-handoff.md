# Crowns Adrift: Spice CAD asset handoff

Reviewed 2026-10-23 against the current Spice CAD exports and the local
`/home/ethanthoma/projects/tribute` checkout.

## Readiness

The original farmstead passed a Crowns Adrift sprite-baker trial: at matching
dimensions its 128×128 RGBA sprite matched the procedural model pixel-for-pixel
on the tested RTX 3070 Vulkan renderer. The trial used a narrow loader-validation
workaround; strict-loader acceptance and the newer orientation settings still
need end-to-end verification. Infantry remains an import and art-direction trial,
not an approved animated character. See the game's `docs/spice-farmstead-trial.md`
at integration commit `8d93fdf` for the observed scope.

Crowns Adrift currently displays troop and building PNGs; its diorama example
authors 3D geometry and renders those sprites. Loading these GLBs into either
that baker or live gameplay requires integration on the game side. This handoff
does not change the game.

## Files to hand over

The original audited exports are in Spice CAD's ignored `output/crowns-adrift-handoff/`:

| Asset | Editable source | GLB | Triangles | GPU vertices | Mesh parts / materials |
| --- | --- | --- | ---: | ---: | ---: |
| Original infantry | `examples/infantry.luau` | `infantry/model.glb` | 966 | 1,018 | 11 / 3 |
| Original farmstead | `examples/farmstead.luau` | `farmstead/model.glb` | 44 | 90 | 4 / 3 |

The GLBs are 46,260 and 7,252 bytes respectively. Each folder also contains
`model.luau` and `GAME_ASSET.md`. Keep the complete source bundle, including
`module-*.luau` when present. `mesh-audit.json` beside the folders records the
measured bounds and per-part checks. Generated output is not Git-tracked.

The separate private knight reconstruction has 1,060 triangles, 11 mesh parts,
and one material. Its source and export live under
`~/.local/share/spice-cad/references/thronefall/`. It is a reconstruction of the
installed game's model, not the original infantry example. Keep that study
separate from the game's production asset library; use it to inform original
weapon designs rather than silently treating it as studio-authored equipment.
The original infantry currently carries a spear and shield, not that sword.

## Re-export from Spice CAD

Run from the Spice CAD repository. Each destination must be new:

```sh
nix develop -c cargo run --locked -- --export-game examples/infantry.luau output/infantry-v2
nix develop -c cargo run --locked -- --export-game examples/farmstead.luau output/farmstead-v2
```

Use GLB for game interchange. STEP/STL are fabrication exports and do not carry
the same materials and attachment hierarchy. Keep the Luau source as the
authoritative model; change dimensions and equipment there before re-exporting.

## Coordinates, scale, and pivots

GLB positions are right-handed, **metres, Y up**. The exporter already converts
CAD millimetres with `(x, y, z) → (x, z, -y) / 1000`; do not convert them again.
The infantry's shield/front is toward GLB **+Z**. Align that direction with the
game's facing convention once on an import wrapper.

Spice CAD now exposes **Model → Orientation & game export…**. Choose the
**Crowns Adrift** preset for model −Y forward / +Z up mapped to game +Z forward /
+Y up, with ground alignment enabled. Set an asset-appropriate uniform scale,
then **Apply** and save. These values are stored in `cad.model_options` in the
Luau source. The exported **Game export** root carries the conversion, scale,
and ground offset; preserve that node instead of repeating its transform on
the game wrapper. Disable grounding for standalone equipment whose origin is
its grip. Literal named attachment transforms can also be edited in this panel.

| Default export | World-space size, X × Y × Z in metres |
| --- | --- |
| Infantry, equipped | 0.037532 × 0.050600 × 0.021996 |
| Farmstead | 0.115000 × 0.115500 × 0.100000 |

These are miniature-scale authoring models. Choose a uniform root scale for the
game, not separate axis scales. For example, a 1.2-unit-tall infantry needs
`1.2 / 0.0506 ≈ 23.7154`; a 1-unit-wide farmstead including its roof needs
`1 / 0.115 ≈ 8.6957`. The tested farmstead instead needs **scale 10** for the
existing art's **1.15-unit roof width**. The infantry height remains an example,
not an approved gameplay dimension.
Apply that scale either in the export settings or on import, never both.

Both original examples have their lowest geometry at Y=0. Origins and local
mesh coordinates are preserved, not automatically centred. The private knight's
posed sword extends below its root, so that study needs separate ground fitting.
Use a wrapper for scale, facing, and ground offset; retain the authored hierarchy.

Infantry nodes include `Unit rig`, `Body pose`, `Head mount`, `Left grip`, and
`Right grip`. These are named transforms, **not bones or exported animation**.
Equipment follows its grip. For reusable standalone weapons, author a grip
origin and an explicit forward/up convention; do not assume every mesh origin
is already a useful pickup or attack pivot.

## Import route for the current game

1. Add GLB loading to the diorama baker. The client uses Bevy 0.19 with default
   features disabled; its current features include scene support through `2d`
   and PBR, but not `bevy_gltf`. Add the loader feature and wait for the imported
   scene, dependencies, and render pipelines before capturing.
2. Replace the relevant procedural asset spawn with the imported hierarchy,
   using a wrapper for the agreed scale and facing. Share loaded meshes and
   materials between instances.
3. Match the existing sprite framing: ordinary units/buildings use 128×128 RGBA
   targets and a 1.65-unit orthographic vertical span. Troops use front/back
   camera offsets `(2.5, 4, ±7)` around a focus 0.59 units above ground. Keep
   pose frames aligned to the same ground anchor and camera.
4. Produce `units/{kind}.png`, `{kind}-step-left.png`, `{kind}-step-right.png`
   and their `-back` equivalents for `slave`, `levy`, and `commander`, as needed.
   The client mirrors these for right-facing units. The farmstead target is
   `buildings/farmstead.png` under `crates/client/assets/`.
5. Preserve the baker's transparent-background and alpha-unpremultiplication
   handling. Inspect the actual 48-pixel troop display, selection state, and
   combat motion before replacing approved sprites.

The existing `DIORAMA_EXPORT` path exports the baker's current procedural
models; setting it alone does not make the baker import Spice CAD models.

For a future live-3D route, use the same GLB hierarchy, but implement gameplay
animation, collision/selection shapes, visibility, and material treatment there.
The sprite path does not need runtime mesh LODs or mesh colliders.

## Geometry and appearance assessment

Fresh GLB checks found **zero degenerate triangles, invalid closed-edge
incidences, or face-winding/normal disagreements** in either original asset or
the private knight study. Normals and positions were inspected from the exported
buffers, not inferred from a screenshot. This is not an exhaustive
self-intersection or cross-part overlap test; closed parts intentionally overlap.

The infantry has 44 triangles with an angle below 5°: helmet 22, shield 10,
spear shaft 12. The farmstead has eight on its thin door sides. These are mostly
expected long/thin CAD faces. Triangles and hard-normal vertex splits are normal
for rigid runtime meshes; an all-quad retopology pass would not inherently
improve them. Inspect the helmet's skinny cut faces for shading artifacts at
game scale. Review topology specifically for deformation if these surfaces
need to bend.

No UVs, textures, tangents, collision proxies, LODs, skeletons, skin weights,
morph targets, or animation clips are generated. Flat palette materials do not
need UVs; texture-based heraldry and baked detail need a UV workflow. The exported material
subset includes base colour, metalness, roughness, and supported specular/IOR
mapping. OpenPBR metadata is not a full OpenPBR shader. Spice CAD's toon preview
is not exported.

The infantry needs gameplay poses and faction/role readability. Detached hands
and a robe-like body can work for this pawn style; they do not provide walking
legs or deforming arms. Develop Bronze Age equipment and palette variants for
the game's art direction. Add shield emblems, straps, and weapon accents only
where they survive the final camera size. The farmstead is intentionally sparse;
judge door/roof contrast and silhouette before adding tiny architectural detail.

## Performance and acceptance

Triangle count is not the immediate concern. A live-3D army of 4,096 copies of
the infantry would submit about 3.96 million triangles per full geometry pass
before culling/LOD, and contains 45,056 mesh-part instances before batching.
That is not a measured draw-call count. Share asset handles, preserve movable
attachments, consider merging static pieces by material, and profile CPU entity
cost, batching, shadows, and GPU time at the intended army size. For sprites,
the GLB cost is paid during baking instead.

Before shipping, the game team should verify:

- GLB loading, required material extensions, facing, scale, and ground contact.
  The farmstead declares `KHR_materials_ior` and `KHR_materials_specular`; do not
  silently discard required extensions. Its specular-free look can also be
  deliberately recreated with game-owned materials.
- Silhouette, faction recognition, and colour contrast at actual gameplay zoom.
- Idle, movement, attack, hit, and death behaviour appropriate to the chosen
  sprite or rigid-animation approach; stable grip and shield attachment.
- Simple gameplay collision/selection bounds and any required standalone pivots.
- Representative crowd performance for live 3D, or sprite framing/alpha for baking.
- Asset provenance and final art approval recorded beside the delivered files.

Code references: Spice CAD's `crates/spice-cad/src/game_asset.rs` and `export.rs`;
Crowns Adrift's `crates/client/src/units.rs`,
`crates/client/examples/diorama.rs`, and `examples/diorama/assets.rs`.

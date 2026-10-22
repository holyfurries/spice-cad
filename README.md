# Spice CAD

Native CAD driven by typed Luau scripts, by [Holy Furries Studio](https://holyfurries.com).
Write shapes in code, adjust parameters in the UI, and export parts for 3D printing
or simple game assets. Built with Rust, SMLib, and egui/wgpu.

![Spice CAD showing the rounded cube's source, live model, and size and corner-radius sliders](docs/images/workspace.png)

## Start

On Linux, with [Nix](https://nixos.org/download/) and flakes enabled, run from this checkout:

```sh
nix run .#spice-cad
```

The first launch builds the app. It opens a rounded cube; choose another
model under **File → Examples**, or use **File → Open** for your own `.luau` file.
To install it: `nix profile install .#spice-cad`.

[Experimental Linux downloads and release instructions](docs/releases.md)
cover running an AppImage without Nix.

## Model with code

```lua
--!strict
local width: number = cad.number(80, {
    label = "Width", min = 20, max = 120, unit = "mm",
})
local plate = cad.box({ width = width, depth = 50, height = 8 })
cad.show("Plate", cad.fillet(plate, { radius = 2 }))
```

`cad.number` exposes a property; adding `min` and `max` gives it a slider.
Edits update the preview automatically. All model lengths are millimetres.

- Build solids with booleans, extrusions, revolutions, fillets, and chamfers.
- Name faces, inspect dimensions, and add holes, pockets, and bosses.
- Edit with Vim-style keys, syntax highlighting, completion, and diagnostics.
- Give parts separate assembly and print placements without rebuilding geometry.
- Share named materials using a documented [OpenPBR parameter subset](docs/modeling.md#materials).

Drag to orbit, Shift-drag to pan, and scroll to zoom. Click a face to inspect it;
press **F** over the viewport to frame the model.

## Print or use in a game

| Four-part spice rack                                                                           | Multipart rocket engine                                                                    |
| ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| ![Exploded spice rack with four colored parts and dovetail joints](docs/images/spice-rack.png) | ![Rocket engine with a chamber, nozzle, and mounting plate](docs/images/rocket-engine.png) |
| [Rack source](examples/rack-parts.luau)                                                          | [Engine source](examples/rocket-engine.luau)                                                 |

For a small game-art example, open **File → Examples → Low-poly farmstead**.
The [farmstead study](docs/modeling.md#low-poly-game-asset-study) compares its
44-triangle GLB with the original Crowns Adrift asset rendered in Bevy.

Use **File → Export**:

- **3D print · STEP + STL** writes separate parts in their print placements.
  Import STLs into your slicer as millimetres. The rack example splits into four
  parts for the Bambu Lab P1S; [printing and assembly notes](docs/printing.md#spice-rack-for-the-p1s)
  cover joint fit samples and orientation.
- **Game asset · GLB** writes the assembly with named parts and colors.

## Develop

```sh
nix develop
cargo run --release --locked
cargo test --locked
```

The native app is the default Cargo target. This is an actively developed tool;
the Nix package currently targets Linux.
Independent OpenCascade checks live in test-only tooling; see
[kernel verification](docs/development.md#independent-geometry-verification).

The workspace contains the application in `crates/spice-cad/`, independent test
tooling in `tools/verification/`, and Luau models in `examples/`. See
[development](docs/development.md) for build commands and architecture.

## Guides

- [Editor and controls](docs/editor.md)
- [Modeling, measurements, and modules](docs/modeling.md)
- [Printing, fit calibration, and the spice rack](docs/printing.md)
- [Development and verification](docs/development.md)
- [Linux downloads and releases](docs/releases.md)

The [typed CAD API](crates/spice-cad/types/cad.luau) and [example models](examples/) are the
reference for writing scripts. Profiling commands and measured evidence live in
[benchmarks/](benchmarks/README.md).

## License

Code and examples: [MIT](LICENSE). Artwork and branding: [separate terms](ASSETS.md).
Dependency attributions: [NOTICE](NOTICE).

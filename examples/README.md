# Example models

Start with the rounded cube. Open another model through **File → Examples** or
**File → Open**. All dimensions are millimetres.

| Model | What it demonstrates |
| --- | --- |
| [Rounded cube](cube.luau) | Basic solids, fillets, and parameter sliders. |
| [Mounting bracket](mounting-bracket.luau) | Booleans, named faces, fillets, and mounting holes. |
| [Curved loft](curved-loft.luau) | Smooth circular sections and rounded profile transitions. |
| [Smooth sweep](smooth-sweep.luau) | A hollow elbow with a controlled bend radius. |
| [Reusable modules](modular/main.luau) | Typed functions, relative imports, and nested modules. Open `main.luau`. |
| [Spice rack](rack-parts.luau) | Four printable parts, fit controls, and separate assembly/print layouts. |
| [Farmstead](farmstead.luau) | A small low-poly game asset with materials and export metadata. |
| [Infantry](infantry.luau) | Proportions, equipment, named poses, attachment frames, and body references. |

Run a model check from the repository root:

```sh
nix develop -c cargo run --locked -- --check-model examples/cube.luau
```

Use **File → Export** for STEP/STL, a game GLB, or named pose GLBs. See
[printing](../docs/printing.md) and [pose export](../docs/pose-exports.md) for
workflow-specific checks. These are starting points; validate fit and strength
before making functional printed parts.

Focused geometry cases used by tests live in
[`crates/spice-cad/tests/fixtures/`](../crates/spice-cad/tests/fixtures/).
They are regression inputs rather than additional public examples.

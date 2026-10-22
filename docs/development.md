# Development and verification

[← Spice CAD](../README.md) · [Releases](releases.md) · [Benchmarks](../benchmarks/README.md)

## Repository layout

| Path | Responsibility |
| --- | --- |
| `crates/spice-cad/` | Native application, Rust examples, artwork, typed Luau API |
| `tools/verification/` | Independent OpenCascade fixtures and packaged-app adapter |
| `examples/` | Luau models bundled into the app, including the four-part rack |
| `benchmarks/` | Profiling commands and retained timing evidence |
| `docs/` | Editor, modeling, printing, development, and release guides |
| `licenses/` | Dependency notices and upstream license material |

Cargo defaults to `spice-cad`; `cargo run` launches its window and ordinary tests
exclude OpenCascade. Generated files belong in ignored `output/` or `target/`.
The repository uses an inverted `.gitignore`; add an allowlist entry when adding
a new kind of source file. `.envrc` and `AGENTS.md` stay untracked.

## Build and check

Use the pinned Nix shell; Rust 1.98 and the native kernel are supplied there:

```sh
nix develop
cargo run --release --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
stylua --check --syntax Luau examples crates/spice-cad/types
nixfmt --check flake.nix
```

The native test suite discovers every top-level Luau example and each example
directory's `main.luau`, resolves imports, and builds its default geometry. It
checks that every part has a closed valid mesh. Keep this gate passing when
changing the public CAD API or adding examples; CI runs it through `cargo test`.

`nix develop .#format` provides lightweight formatting and workflow tools without
building the CAD kernel. Use it for `actionlint` and `yamlfmt -lint
.github/workflows/build.yml`. Format changed Rust with `cargo fmt --all`, Luau
with `stylua --syntax Luau examples crates/spice-cad/types`, and Nix with `nixfmt`.
Match existing Rust naming and indentation. Include units in dimensional names,
propagate operational errors with `Result`, and bound geometry and worker budgets.

Tests are inline Rust `#[test]` cases. Add behavior-focused regressions for
geometry, editor history, cache invalidation, and export validation. There is no
numeric coverage target. `.cargo/config.toml` runs tests serially because SMLib's
global context can contend with worker response deadlines. Full workspace tests
require the verification shell and a built app.

## Application boundaries

`src/main.rs` connects the document, worker, and interface. `model.rs` owns Luau
recipes, `kernel.rs` the SMLib boundary, `viewport.rs` GPU rendering, and `mesh.rs`
mesh validation. `text_buffer.rs` owns the rope and history; syntax, analysis,
file I/O, and geometry work run on separate workers with bounded pending work.
SMLib handles remain on the serialized geometry worker.

Geometry, placement, and display state have separate cache identities. Moving
finished parts updates transforms and reuses canonical meshes. Parameter edits
can reuse unchanged operations; geometry changes invalidate dependent results.
Numeric edits reuse a type check only when all other source bytes are unchanged;
assertions and design checks still run. Obsolete work stops between operations,
while a kernel call already in progress finishes before cancellation takes effect.

Interactive geometry previews use coarser display meshing and defer refinement
until 150 ms after parameter edits settle. Exports request final preparation.
Camera motion and four-view layout changes reuse geometry buffers. Keep tests
for these structural properties rather than timing assertions on shared runners.

## Independent geometry verification

Build the app, then opt into the test-only OpenCascade suite:

```sh
nix develop -c cargo build --locked
nix develop .#verification -c cargo test --locked -p spice-cad-verification
nix develop .#verification -c cargo clippy --locked -p spice-cad-verification --all-targets -- -D warnings
```

The verifier invokes `target/debug/spice-cad`. Set `SPICE_CAD_BIN` to an absolute
path for another build. `SPICE_CAD_ARTIFACTS` chooses an absolute fixture directory.
Failures retain sources, available exports, and command logs; successful fixtures
are removed after all assertions pass. See [release checks](releases.md#build-and-validate)
for testing the packaged download in CI.

The suite checks imported B-reps with OCCT's standard geometric validator,
measures surface volume using adaptive Gauss–Kronrod integration across B-spline
spans, and independently audits mesh closure, bounds, part counts, and placements.
Non-finite volume, failed integration, or estimated relative error above `1e-8`
fails. Mesh volume must agree with surface volume within 0.2%.

Independent rack fixtures check joints, jar lifting space, fit samples, and
support-free orientations. Cavity and bushing fixtures compare analytic volume,
section area, missing/extra section area, and sampled boundaries within 0.001 mm.
Curved analytic volume and section-area checks allow 0.01% relative error.
Negative fixtures cover misplaced cavities, self-intersecting B-reps, and open
surfaces without closed volume.

These checks do not prove general shape equivalence. STEP import can repair a
shape before validation. OCCT's optional exact validator reports curve/parameter
inconsistencies on some curved imports that its standard checker accepts.
Keep that limitation separate from physical print fit and strength.

## Kernel and dependency updates

The application uses pinned SMLib sources. OCCT 7.8.1 and opencascade-rs 0.3.0 are
confined to `tools/verification/` and `.#verification`; neither is an application
dependency. Update kernels and bindings together and rerun both test suites and
exports. Local binding overrides belong in workspace `[patch.crates-io]` entries.

Dependency sources are pinned in `Cargo.lock` and `flake.lock`. License inventory
commands and redistribution obligations live in [licenses](../licenses/README.md).
Artwork terms and the shared splash's provenance live in [ASSETS.md](../ASSETS.md)
and the [brand bundle](../crates/spice-cad/assets/brand/README.md).

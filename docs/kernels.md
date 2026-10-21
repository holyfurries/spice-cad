# Kernel evaluation

[← Spice CAD](../README.md) · Commands run from the repository root.

## SMLib browser evaluation

This records the browser evaluation against the original OpenCascade rack CLI.
The native application now uses SMLib; the reference CLI remains available.

`evaluation/smlibwasm.html` recreates the default front-left part with
[`@classcad/smlibwasm`](https://github.com/awv-informatik/smlibpy/tree/main/js)
0.3.0. It includes both dovetail directions, the print orientation, STEP/STL
downloads, two rendered views, and mesh/STEP checks. The reference rack generator
uses Rust and OpenCascade.

The October 14, 2026 evaluation passed on Linux with a Ryzen 9 5900X and
Chromium 154. OpenCascade imported the SMLib STEP and measured zero extra or
missing volume against the existing part. Both measured 95 × 174 × 161 mm and
203,856 mm³. Their rendered previews were byte-identical; both STLs had 200
triangles, closed consistently oriented meshes, and no overhangs beyond 45°.

Warm median construction times were **149.5 ms for SMLib WASM** and **317.6 ms
for the equivalent native OpenCascade build in release mode**. Each backend ran
five builds; the first was excluded. Import, export, rendering, compilation, and
process startup were excluded. SMLib's initial module import over local HTTP
took 294.5 ms. These are measurements of one planar part, not a general kernel
benchmark. Curved geometry, advanced fillets, female-joint samples, the complete
assembly, parameter sweeps, and MCP were not evaluated.

`evaluation/results.json` contains the raw measurements, checks, source hashes,
and limitations. Generated exports, preview PNGs, and the captured native
benchmark harness are in `output/evaluation/`. The candidate STEP is 57,078
bytes versus 187,550 bytes for the reference. STL size is not directly
comparable: SMLib emitted binary STL and the reference is ASCII.

To replay the browser checks from the repository root, first generate the
reference files, download the pinned package, and serve the page over HTTP:

```sh
nix develop -c cargo run --locked -p spice-rack
mkdir -p output/evaluation/vendor
nix-shell -p curl --run 'curl -fsSL https://registry.npmjs.org/@classcad/smlibwasm/-/smlibwasm-0.3.0.tgz -o output/evaluation/smlibwasm-0.3.0.tgz'
echo 'a0b960e783b3f5e33fe46c584fea828df2356a10595316780574c92ab455f9bc  output/evaluation/smlibwasm-0.3.0.tgz' | sha256sum --check
nix-shell -p gnutar --run 'tar -xzf output/evaluation/smlibwasm-0.3.0.tgz -C output/evaluation/vendor'
nix-shell -p miniserve --run 'miniserve . --interfaces 127.0.0.1 --port 8767'
```

Open `http://127.0.0.1:8767/evaluation/smlibwasm.html` and select **Run
evaluation**. The browser page uses fixed default dimensions; update it if the
Rust design changes. A browser prototype is supported by these results;
migrating the production generator requires checking the remaining parts and
behaviors.

## Modify the CAD engine

Both the bindings and kernel are available as source. The bindings use LGPL-2.1;
OpenCascade uses LGPL-2.1 with its additional exception. Their license files
govern changes and redistribution.

To work on the Rust bindings, clone them beside this project from the Nix shell:

```sh
git clone https://github.com/bschwind/opencascade-rs ../opencascade-rs
git -C ../opencascade-rs checkout 32758df23a137f4e7c786c618b0317fc3badb3b2
```

Add this section to `Cargo.toml`:

```toml
[patch.crates-io]
opencascade = { path = "../opencascade-rs/crates/opencascade" }
opencascade-sys = { path = "../opencascade-rs/crates/opencascade-sys" }
```

Then run `cargo update -p opencascade -p opencascade-sys`,
`cargo test --locked`, and `cargo run --locked -p spice-rack`. The `builtin`
feature stays disabled, so no OCCT submodule is needed. Commit the changed
lockfile when you intentionally adopt changes in your own version control
workflow.

`flake.lock` pins the tools and kernel; `Cargo.lock` pins Rust dependencies. The
released 0.3.0 bindings require OCCT 7.8 APIs, so the kernel comes from the
Nixpkgs 24.11 input while Rust tools come from the current pinned Nixpkgs input.
To modify the C++ kernel, change the `occt` derivation in `flake.nix` to build
your checkout. Keep kernel and binding API changes coordinated.

# Dependency license materials

Keep this directory, the root `LICENSE` and `NOTICE`, and the brand terms with
distributed builds. Nix installs them under `share/doc/spice-cad/`.

`RUST-DEPENDENCIES.txt` is generated from the native application's dependency graph
in `Cargo.lock`, including build dependencies, for x86-64 and ARM64 Linux. It is a
conservative inventory, not a claim that every listed crate is linked into
each executable. Native kernel, font, and Unicode notices are collected
separately because Cargo metadata does not fully describe their contents.

## Updating Rust notices

From the repository root:

```sh
nix develop
cargo-about generate --manifest-path native/Cargo.toml --all-features --locked --offline --fail --config about.toml licenses/about.hbs --output-file licenses/RUST-DEPENDENCIES.txt
```

Review license changes whenever `Cargo.lock`, bundled fonts, or native inputs
change. The report links archives with missing license files to the supplemental
copies below. Update those copies and this mapping when versions change; a
successful generator exit alone is not a copyright-notice completeness check.
Do not replace upstream notices with the project's MIT license.

## Crates whose archives omit license files

| Component                                                         | License material                                                                                                                          |
| ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Spice CAD                                                         | [Project MIT license](../LICENSE)                                                                                                         |
| accesskit 0.25.1                                                  | [accesskit.txt](upstream/accesskit.txt)                                                                                                   |
| dpi 0.1.2                                                         | [Apache license](upstream/dpi.txt), [libm MIT license](upstream/dpi-libm-MIT.txt), [libm copyright notices](upstream/dpi-libm-NOTICE.txt) |
| ecolor, eframe, egui, egui-wgpu, egui-winit, emath, epaint 0.37.0 | [egui-MIT.txt](upstream/egui-MIT.txt)                                                                                                     |
| libm 0.2.16                                                       | [libm.txt](upstream/libm.txt), including its additional notices                                                                           |
| lua-src 551.0.2                                                   | [lua-src.txt](upstream/lua-src.txt)                                                                                                       |
| mlua-sys 0.13.0                                                   | [mlua-sys.txt](upstream/mlua-sys.txt)                                                                                                     |
| opencascade / opencascade-sys 0.3.0                               | [opencascade.txt](upstream/opencascade.txt), [opencascade-sys.txt](upstream/opencascade-sys.txt)                                          |
| opencascade-kicad-parser 0.3.0                                    | [Published SPDX expression and attributed MIT text](upstream/opencascade-kicad-parser.txt)                                                |
| profiling 1.0.18                                                  | [profiling.txt](upstream/profiling.txt)                                                                                                   |
| spirv 0.4.0+sdk-1.4.341.0                                         | [spirv.txt](upstream/spirv.txt)                                                                                                           |
| tree-sitter 0.25.10                                               | [tree-sitter.txt](upstream/tree-sitter.txt), [Unicode notice](tree-sitter/unicode-LICENSE.txt)                                            |
| tree-sitter-luau 1.2.0                                            | [tree-sitter-luau.txt](upstream/tree-sitter-luau.txt)                                                                                     |
| type-map 0.5.1                                                    | [type-map.txt](upstream/type-map.txt)                                                                                                     |

`upstream-sources.tsv` records the source URL used for each supplemental
upstream file. The egui family shares the repository's MIT license, copied from
the immutable eframe 0.37.0 publication revision. Some sibling crate archives
record an unavailable Git revision; their published MIT declaration and common
repository ownership were checked against that license. The parser's archive
omits both a separate license file and a Git revision; its file explicitly
identifies its published license declaration and author attribution.

## Other bundled components

- Experimental AppImages add nix-appimage's MIT-licensed AppRun at revision
  `7946addbc0d97e358a6d7aefe5e82310f0fe6b18` and the MIT-licensed type2 runtime at
  `01164bfcbc8dd2bd0d7e3706f97035108d6b91ba`. Their notices are in `packaging/`.
  The runtime's statically linked compression/FUSE libraries and the bundled
  system-library closure retain their own terms; review them before publishing
  a release, as described in [release instructions](../docs/releases.md).
- SMLib / SM_API and AWV's patches use the source pinned in `flake.lock`:
  `735403a40b261b71967a59d670cd7970f032d04e`. Their Apache-2.0 and MIT materials,
  upstream notice, and patch descriptions are copied here. The unabridged
  upstream notice also describes Python/font components outside this build.
- oneTBB materials are copied from the pinned 2022.3.0 source, including its
  third-party notices.
- The embedded VM license is copied from `luau0-src 0.22.0+luau740/LICENSE`.
- `fonts/` copies every `.txt` license/notice from
  `epaint_default_fonts 0.37.0/fonts/`, including the Hack/Bitstream, Noto, Ubuntu,
  and emoji/icon notices. These fonts retain their original licenses.
- OpenCascade 7.8.1's LGPL text and exception apply to the separate test-only
  verification package. Its source is pinned in the verification Nix shell; [rebuild instructions](../docs/kernels.md)
  explain how to modify the reference libraries. The default application does
  not link OpenCascade.

Preserve the dependency sources, build scripts, notices, and applicable source
or relinking rights when distributing optional LGPL components or a complete
Nix/system-library closure. These application notices do not replace the
license materials supplied by those separate system packages.

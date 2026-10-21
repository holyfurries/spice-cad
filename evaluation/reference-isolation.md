# Native and independent verification split

The application now has one modeling path: typed Luau recipes evaluated by native
SMLib. The root Cargo manifest is a virtual workspace whose default member is
`native/`. Removed the spice-rack binary, legacy rack controls and worker commands,
parallel Rust/SMLib rack builder, and user-facing rack-tools feature/package.
The normal camera initializer now takes generic model dimensions.

`verification/` has only dev dependencies and compiles its reference code under
`cfg(test)`. It retains the independent OpenCascade rack construction and eight
geometry safety tests, plus four production-executable verification tests. Those
invoke the built `spice-cad` CLI and reopen STEP exports with OpenCascade. Cases
cover curved geometry, enclosed cavities, selected fillets/chamfers, revolutions,
lofts, sweeps, rack parts, multipart game models, and separate print placements.
Imported meshes are checked for closure/orientation, dimensions, and signed volume.

Validation after the split:

- 263 native tests passed; two existing tests remain ignored.
- 12 independent verification tests passed, including retained jar/lift clearance,
  print orientation, dovetail movement, fit-sample, and cabinet-limit checks.
- Native and verification Clippy passed with warnings denied.
- Native build, rustfmt, nixfmt, actionlint, and workflow formatting passed.
- Cargo's full native dependency tree, including dev/build dependencies, contains
  neither OpenCascade nor the verification package. ELF dependencies likewise
  contain no OpenCascade libraries.
- Evaluated Nix derivations confirm only the explicit verification shell sets
  `DEP_OCCT_ROOT`; default development and packaged application do not.
- The native-only development shell launched the app in an isolated Xvfb/Vulkan
  session and exercised four views, navigation, collision checks, and resizing.

Normal CI checks use SMLib only. Independent verification runs in a separate
version-tag job and gates AppImage packaging. Its Nix cache is separate, while
Cargo dependencies use the same shared cache key as native CI. User-facing docs
now describe Luau export; historical evaluation reports retain their measurements.

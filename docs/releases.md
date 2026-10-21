# Linux downloads and releases

Spice CAD is experimental. Downloads currently target x86-64 Linux; Windows,
macOS, and ARM64 downloads are not yet tested. Nix builds remain available for
x86-64 and ARM64 Linux.

## Run a download

Extract the release archive, keeping its license materials beside the app.
From the extracted directory:

```sh
sha256sum -c SHA256SUMS
chmod +x spice-cad-x86_64-linux.AppImage
./spice-cad-x86_64-linux.AppImage
```

Nix is not required. The AppImage includes the CAD kernel, analyzer, language
server, and runtime libraries. It requires enabled Linux user namespaces and a
working desktop graphics driver. If FUSE mounting fails, try:

```sh
./spice-cad-x86_64-linux.AppImage --appimage-extract-and-run
```

Extraction uses temporary disk space and takes longer to start. It does not
bypass disabled user namespaces. Graphics compatibility, particularly proprietary
drivers, still needs testing on real desktops. Report your distribution, GPU,
driver, and terminal error with startup failures.

## Build and validate

```sh
nix develop -c cargo fmt --all -- --check
nix develop -c cargo clippy --locked --workspace --all-features --all-targets -- -D warnings
nix develop -c cargo test --locked --workspace --all-features
nix build .#spice-cad-appimage
./result --appimage-extract-and-run --check-model models/mounting-bracket.luau
```

CI runs these checks for pull requests and pushes to `main`, then uploads a
download artifact with SHA-256 checksums and the exact application source.
A container with no Nix installation checks that the AppImage can analyze,
construct, and tessellate a model using its bundled dependencies. This is a
headless check; it does not establish desktop or graphics-driver compatibility.

## Cut a release

Push a version tag, such as `v0.1.0`, on a reviewed commit. The same checks run
before a **draft** GitHub release is created. Draft creation does not change
repository visibility or publish a release. Update the Cargo package versions
when changing versions; the AppImage name follows the native package version.

Before publishing the draft, test its actual download on a separate Linux
desktop: launch it, open a model, edit a parameter, save/reopen, export STEP/STL
and GLB, and check graphics and text rendering. Review all bundled runtime
license notices and source/relinking obligations when distributing the full
library closure. Keep the source archive and pinned Nix inputs available.

The AppImage packager is pinned in `flake.lock`:
[nix-appimage](https://github.com/ralismark/nix-appimage). Its documented namespace
and graphics limitations also apply to these experimental downloads.

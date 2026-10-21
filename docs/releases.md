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
nix develop .#format -c cargo fmt --all -- --check
nix develop -c cargo clippy --locked --all-targets -- -D warnings
nix develop -c cargo test --locked
nix develop -c cargo build --locked
nix develop .#verification -c cargo test --locked -p spice-cad-verification
nix build .#spice-cad-appimage
./result --appimage-extract-and-run --check-model examples/mounting-bracket.luau
```

CI runs formatting in a lightweight shell without building the CAD kernel.
Pull requests and pushes to `main` run native lint and tests when native code,
models, dependencies, or CI configuration change; documentation-only changes
skip native builds. Nix tools/kernel and Cargo dependencies have separate caches.
CI disables debug symbols and incremental compilation and uses lld for test
linking; production release optimization is unchanged. Native tools are cached
before Rust compilation so a later failure does not discard that build.
These jobs use SMLib without OpenCascade. On version tags, packaging runs in
parallel with native lint and tests. A separate verification job downloads that
exact AppImage artifact, checks its SHA-256 checksum, and extracts it once in a
network-isolated container with no host Nix store. The full independent suite
invokes the packaged app through `tools/verification/src/bin/packaged-app.rs` and
reopens its STEP exports with OpenCascade outside the container. Checks include
B-rep validity, surface volumes, cross-sections, closed meshes, dimensions, part
counts, print placements, and independent rack fixtures. Native checks, packaging,
and packaged verification must all pass before draft release creation. Verification
has a separate Nix cache; the Cargo dependency cache is shared with native checks.

Failed verification uploads `spice-cad-verification-failure` for 14 days. It
contains the suite log, extraction/container logs, and failed fixture directories
with their Luau source, command stdout/stderr, and available exports. Successful
fixtures are removed. Set `SPICE_CAD_ARTIFACTS` to an absolute directory to choose
where local failures are retained; otherwise fixtures remain in the test process's
temporary directory. The console failure identifies the fixture or command path.

AppImages are built only when a `v*` version tag is pushed.
These runs upload a download artifact with SHA-256 checksums
and the exact application source. A container with no Nix installation checks
that the AppImage can analyze, construct, and tessellate a model using its bundled
dependencies. This is a
headless check that also exports STEP, STL, and GLB; it does not establish desktop
or graphics-driver compatibility.
The isolated test container runs as root with mount-namespace capability to avoid
Ubuntu's restrictions on nested unprivileged UID mappings. It has no network and
read-only access to the app and models.

## Cut a release

Push a version tag, such as `v0.1.0`, on a reviewed commit. The same checks run
before a **draft** GitHub release is created. Draft creation does not change
repository visibility or publish a release. Update `crates/spice-cad/Cargo.toml` when
changing the application version; the AppImage name follows that version.

Before publishing the draft, test its actual download on a separate Linux
desktop: launch it, open a model, edit a parameter, save/reopen, export STEP/STL
and GLB, and check graphics and text rendering. Review all bundled runtime
license notices and source/relinking obligations when distributing the full
library closure. Keep the source archive and pinned Nix inputs available.

The AppImage packager is pinned in `flake.lock`:
[nix-appimage](https://github.com/ralismark/nix-appimage). Its documented namespace
and graphics limitations also apply to these experimental downloads.

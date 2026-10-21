# Browser prototype

[← Spice CAD](../README.md) · Commands run from the repository root.

`browser/index.html` is a local rack configurator using SMLib WASM 0.3.0. Adjust
dimensions and joint clearance to rebuild all four rack parts and both joint fit
pairs. Inspect the assembled rack, an exploded assembly, or each part's print
orientation with orbit, pan, and zoom controls. Download individual STL and STEP
files or a multi-body assembled STEP. The Rust CLI remains available.

The viewport-first workspace has a collapsible parts browser and dimension
inspector, front/right/top/left/isometric camera controls, and dark, light gray,
or system themes. Open command search with Ctrl+K (Command+K on macOS) or S;
type to filter and press Enter to run the first result. Selecting a part shows
its print orientation; dimension edits always rebuild the complete rack. Rack
and fit-sample downloads are under Manufacturing in the inspector.

Name the project in the top bar. Valid configurations autosave in this browser
and restore on reload. Invalid edits leave the last valid autosave intact. Use
**Save project** (Ctrl/Command+S) to download a portable `.spice-rack.json` file
and **Open project** (Ctrl/Command+O) to reopen one. Imports are checked for the
supported format, dimensions, and printable geometry before replacing your
current work. If browser storage is unavailable, use the downloadable project.

**Print pack ZIP** downloads four rack STLs, four joint-fit STLs, the project
file, a dimensions manifest, and `PRINTING.md` with slicing and assembly steps.
Extract it and print the fit samples first. The pack preserves print
orientations; select your actual printer, nozzle, filament, and plate in Bambu
Studio.

Undo and Redo retain up to 32 validated dimension configurations for the current
tab. Invalid or pending edits can be undone to the last valid configuration; a
new valid edit clears the redo branch. Resetting dimensions is undoable. Use the
toolbar, command search, or Ctrl/Command+Z and Ctrl/Command+Shift+Z outside
input fields (Ctrl+Y also redoes). Input fields keep their native text undo.
Undo history is cleared when the page reloads; the last valid project is
restored from autosave. History does not store model bodies.

Download the two pinned dependencies once from the repository root:

```sh
mkdir -p output/browser/vendor/smlib output/browser/vendor/three
nix-shell -p curl gnutar --run '
  curl -fsSL https://registry.npmjs.org/@classcad/smlibwasm/-/smlibwasm-0.3.0.tgz -o output/browser/smlibwasm-0.3.0.tgz &&
  echo "a0b960e783b3f5e33fe46c584fea828df2356a10595316780574c92ab455f9bc  output/browser/smlibwasm-0.3.0.tgz" | sha256sum --check &&
  tar -xzf output/browser/smlibwasm-0.3.0.tgz -C output/browser/vendor/smlib &&
  curl -fsSL https://registry.npmjs.org/three/-/three-0.170.0.tgz -o output/browser/three-0.170.0.tgz &&
  echo "4a608a355dcaba72e0e5383cdc814303f5b6060b43c238cdf6932dceb699238d  output/browser/three-0.170.0.tgz" | sha256sum --check &&
  tar -xzf output/browser/three-0.170.0.tgz -C output/browser/vendor/three
'
```

Then serve the project and open `http://127.0.0.1:8767/browser/index.html`:

```sh
nix-shell -p miniserve --run 'miniserve . --interfaces 127.0.0.1 --port 8767'
```

Modeling, viewing, and exports run locally in the browser using the cached
assets; no CAD service or npm build step is needed. Downloaded parts use their
print orientations. Print both fit pairs before committing to the full rack.

The browser checks cabinet dimensions, jar height and lifting room, P1S part
sizes, mesh closure/orientation, and the 45° overhang limit before enabling
downloads. Invalid or pending inputs disable exports. Some shorter step
configurations need supports with the current joint layout; the browser rejects
them and names the affected part. Increase the rise or step count, or revise the
joint layout, rather than using an unchecked export.

Independent checks imported all eight STEP parts and the assembled STEP into
OpenCascade for both the default configuration and a 300 mm wide, three-step
configuration with a 70 mm rise, 4 mm walls, and 0.3 mm joint clearance. Both
matched their Rust references with zero extra or missing volume per part.
Assembly positioning, part overlap, and 25/15 jar lifting volumes were checked.
`browser/validation.json` records these checks and the browser interaction
tests. Physical printer fit, hinge clearance, and strength remain unverified.

For optional support geometry experiments, see the
[Support Fins assessment](../evaluation/support-fins.md). It records the upstream
workflow, integration constraints, and a proposed print comparison; the fin
engine has not been integrated or physically evaluated on this rack.

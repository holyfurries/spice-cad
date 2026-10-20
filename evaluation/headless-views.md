# Headless inspection views

`spice-cad --render-views SOURCE.luau NEW_DIRECTORY` exports four 800 × 600
orthographic assembly views, a labeled 1600 × 1200 contact sheet, and the exact
source snapshot. Top, front, side, and isometric views preserve part colors and
placements. Faces use a depth buffer; CAD edges use a one-pixel visibility
tolerance to keep lines continuous on sloping surfaces.

The command returns before window/GPU setup and skips inline thumbnail rendering.
It shares the bounded triangle rasterizer with inline thumbnails, retaining the
thumbnail's stack depth buffer and existing pixel budget. Headless rendering
reads canonical vertices through instance transforms without allocating world
meshes. Its four views share a 256-million pixel-visit limit, including edge depth
checks. PNG output uses the already locked `png` dependency, fast compression,
and one preallocated byte buffer per encoded image; no packages were added to
the dependency graph.

## Release checks

Run on 2026-10-19 with the cached Nix environment and Rust 1.98.1:

```sh
cargo build --release --locked -p spice-cad
for model in rack-parts mounting-bracket rocket-engine; do
  time env -u DISPLAY -u WAYLAND_DISPLAY -u XDG_RUNTIME_DIR \
    VK_DRIVER_FILES=/nonexistent target/release/spice-cad \
    --render-views "models/$model.luau" "output/native/headless-views/$model"
done
```

Single-run wall times, including source validation, kernel construction,
tessellation, all images, PNG encoding, and file writes:

| Model | Time |
| --- | ---: |
| Four-part rack | 269 ms |
| Filleted mounting bracket | 464 ms |
| Rocket engine | 199 ms |

These are smoke-test timings on a warm development machine, not latency
percentiles. All three contact sheets were visually inspected for framing,
orientation, holes, part colors, and edge visibility.

The workspace's 158 tests passed. New checks cover camera directions and framing
at different scales, transformed-part occlusion regardless of draw order, hidden
edges, exhausted work budgets, decodable opaque PNGs, exact source retention,
failed measurement checks without creating output, and existing-directory
preservation. Clippy passed for the workspace with all targets/features and
warnings denied; rustfmt and the release build passed.

## Scope

Views fit independently and can have different scales. Rasterized edges are an
inspection aid; these images are not dimensioned drawings. Measurements remain
available in the native viewport. Print layouts, interactive export controls,
antialiasing, and agent integration are outside this slice. Existing output
folders are rejected; errors while writing remove only the new export folder.

# Performance measurements

Build and run from the repository root. Record commit, build profile, adapter,
framebuffer size, and system load with every comparison. Run workloads serially.
These measurements exclude GPU completion and physical presentation unless stated.

## Reproduce current workloads

```sh
nix develop
cargo build --release --locked
target/release/spice-cad --profile
target/release/spice-cad --profile-preview
target/release/spice-cad --profile-rotation
target/release/spice-cad --profile-geometry
cargo run --release --locked --example editor-benchmark
```

`--profile` logs CPU startup stages, the first geometry upload, resize passes,
retained buffer capacity, and geometry/placement upload counts. It also reports
slider latency distributions over the latest 512 accepted revisions. Select a
backend with `WGPU_BACKEND=vulkan` or `WGPU_BACKEND=gl` for controlled comparisons.
Comparisons using different adapters, including software renderers, are invalid.

The bounded translation, rotation, and geometry sweeps close automatically without
changing model files. See [slider methodology and measured comparisons](sliders.md).
Normal UI operation exposes the same counters under **Debug → Performance**.

The editor benchmark constructs ropes, performs 2,000 distributed insertions,
exercises undo/redo, and measures 200 editor frames in a 440 × 800 viewport after
five warm-up frames. It excludes disk I/O, background syntax/LSP work, GPU drawing,
and window startup. A local release run on 2026-10-18 measured:

| Input | Insert p95 | Editor UI frame p95 |
| --- | ---: | ---: |
| 100,000 lines | 6.03 µs | 0.104 ms |
| One million characters on one line | 3.09 µs | 0.144 ms |

These historical observations are not current-release performance guarantees.
Regression tests assert bounded work, buffer reuse, and correct results rather
than machine-specific millisecond thresholds.

## Retained startup evidence

[Raw startup observations](results/startup.json) preserve three historical launches
per executable on an NVIDIA RTX 3070 Vulkan adapter. OS caches were not flushed;
stderr markers were sampled every 10 ms. RSS includes driver mappings, excludes
child-process and GPU allocations, and idle CPU measures one logical core over
two seconds. Startup and resize numbers exclude renderer completion and scanout.

That record describes an earlier application and packaging state. Use the current
profiling commands for new comparisons. Superseded feature reports and the retired
browser prototype remain available in Git history instead of the working tree.

# gltf-json 1.4.1

Source: https://crates.io/crates/gltf-json/1.4.1 (gltf-rs/gltf).
Published crate SHA-256: `e6176f9d60a7eab0a877e8e96548605dedbde9190a7ae1e80bbcc1c9af03ab14`.
Source and manifest are unchanged except for the required-specular whitelist and
an `allow_external_materials_specular` feature. Enable that feature for Bevy
0.19.1: it permits the required declaration and retains the raw extension data
that Bevy parses. Enabling the upstream typed `KHR_materials_specular` feature
instead consumes that data before Bevy's raw extension reader sees it.
No validation is disabled and unknown required extensions remain rejected.

Retain both upstream licenses. Replace this vendored correction with a supported
upstream release when it includes these declarations and passes the loader gate.

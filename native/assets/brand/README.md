# Shared studio splash

Vendored unchanged from [holyfurries/brand-assets](https://github.com/holyfurries/brand-assets/tree/ca733e8c6f2652e8583538b2564bae48334a5585),
commit `ca733e8c6f2652e8583538b2564bae48334a5585`:

- `poster.png` is upstream `splash/poster.png`.
- `manifest.json` is upstream `splash/manifest.json`.
- `LICENSE` contains upstream artwork terms, separate from this application's license.

The app embeds the approved transparent native fallback, centers it over the
manifest's forest background (`#252c24` in sRGB), and preserves its 6:5 canvas.
No product-specific text or jar is added to the startup splash. Escape, Space,
or a click dismisses it. Startup completion or failure releases its texture;
there is no minimum display duration or runtime download.

This pinned upstream bundle provides an animated browser SVG and a static native
poster, but no frame atlases. Spice CAD uses the native poster, including for
reduced motion. Animated native playback awaits a shared baked-frame export;
the browser keyframes are not independently reimplemented here.

To update, copy these three files from a reviewed upstream commit, update this
revision, and run `cargo test --locked -p spice-cad studio::tests`. If the canvas
or background changes, update the native layout/decode bounds and regression
checks in `native/src/studio.rs` together with the asset files.

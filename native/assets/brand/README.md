# Shared studio splash

Vendored unchanged from [holyfurries/brand-assets](https://github.com/holyfurries/brand-assets),
commit `1de8f8f114ac73a0b0cc20e72edd36677c1cf89e`:

- `poster.png` is upstream `splash/poster.png`.
- `manifest.json` is upstream `splash/manifest.json`.
- `LICENSE` contains upstream artwork terms, separate from this application's license.
- `frames/` contains upstream `splash/frames/`, including its playback manifest
  and source SVG SHA-256.

The app embeds the shared 60 fps animation, centers it over the manifest's
forest background (`#252c24` in sRGB), and preserves its 6:5 canvas.
No product-specific text or jar is added to the startup splash. Escape, Space,
or a click dismisses it. Startup completion or failure releases its texture;
there is no minimum display duration or runtime download.

The ten 1920 by 800 PNG pages contain eight 480 by 400 frames each (73 frames
total). PNG decoding runs on a named worker with a one-page channel. Playback
starts when the first page arrives, advances by elapsed time, and holds the
finished logo after 1200 ms. It never loops or delays a ready application. The
renderer reuses one 5.86 MiB atlas texture and uploads only on page changes;
prefetch is bounded to one queued page and one page being decoded. Dismissal
closes the receiver, releasing the texture immediately and allowing the worker
to exit without blocking the UI on a join. Decoding already in progress finishes.

Set `HOLYFURRIES_REDUCED_MOTION=1` before launch to use the approved static
poster. This is an explicit native preference; the app does not currently read
the operating system's reduced-motion setting. Neither a browser nor a new
image/video decoder is needed at runtime. The source keyframes are baked by
the upstream tool rather than independently reimplemented here.

To update, regenerate and validate the upstream bundle, copy the files above
from a reviewed commit, and update this revision. Run
`cargo test --locked -p spice-cad splash` and the workspace checks. If the canvas
or atlas count changes, update the layout/decode bounds, embedded page list,
and regression checks in `native/src/studio.rs` and `native/src/splash_frames.rs`.

# Exact-volume reuse

The pinned SMLib `SmApiQueries.h` documents volume as numerical integration,
with relative accuracy clamped to `1e-8 .. 1e-1`. The mass-properties API shares
its integration core; switching APIs does not establish a faster equivalent
calculation. Increasing the accuracy tolerance or substituting mesh volume
would weaken the independent tessellation check. Both remain unchanged.

Each native body now retains one successful, finite, nonnegative volume query.
The memo occupies a fixed `Option<f64>` per body, requires no heap allocation,
and is destroyed with the body. It preserves the existing `1e-8` relative
accuracy. Preview and game tessellation still validate their own closed meshes
and compare their approximate volumes against this exact solid volume.

| Operation | Memo behavior |
| --- | --- |
| Volume, bounds, edges, face query, tessellation | Reuse |
| Deep body clone | Copy independently |
| Successful translation or rotation | Preserve invariant volume |
| Failed translation or rotation | Discard before entering the kernel |
| Boolean, chamfer, fillet | Discard before attempting geometry changes |
| STEP conversion to NURBS | Discard before conversion |

STEP export therefore continues to compare independently integrated volumes
before and after NURBS conversion. Cached print export reuses the canonical
volume before conversion; game export can reuse it during its separate, coarser
tessellation. A failed mutation cannot leave an earlier value marked valid.
Future shape mutations must explicitly invalidate the memo before entering
SMLib, including error paths.

Focused tests count actual integration calls rather than relying on timings.
A cylinder's repeated query, preview mesh, game mesh, and translated/rotated
deep copy require one integration in total. Independently reintegrating the
transformed copy verifies the invariant against the kernel. Separate checks
exercise subtraction, union, chamfer, rejected treatment, and NURBS conversion;
each requires a fresh query. Assertions run after releasing the kernel context.

```sh
nix develop -c cargo test --locked -p spice-cad --all-features exact_volume_
```

This improves repeated work only. A newly changed filleted body still requires
its original high-accuracy integration; no lower first-build latency is claimed.

## Game-tessellation validation correction

The investigation found that the `0.25 mm` game tessellation used
the same area-based `0.04 mm` error envelope as the `0.02 mm` preview mesh.
A radius-5 mm, height-10 mm cylinder therefore failed game validation: its
60-triangle mesh encloses about `765.272 mm³`, versus exact `785.398 mm³`.
This occurred without memoization as well. A separate correction now derives the
envelope from twice the requested chord tolerance, preserving the previous
preview envelope and matching the game's intentionally coarser geometry.
Independent exact integration remains at `1e-8`; closedness, orientation,
positive volume and the mesh-versus-solid comparison remain mandatory.
A regression covers this cylinder at both tessellation qualities.

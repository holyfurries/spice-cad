# Workspace review and studio identity

## Modeling-tool comparison

Reviewed Plasticity's annotated interface image and documentation for its
[interface structure](https://doc.plasticity.xyz/plasticity-essentials/plasticity-interface/user-interface-overview),
[outliner](https://doc.plasticity.xyz/plasticity-essentials/plasticity-interface/outliner),
[view cube](https://doc.plasticity.xyz/plasticity-essentials/plasticity-interface/view-cube),
and [display controls](https://doc.plasticity.xyz/plasticity-essentials/plasticity-interface/shader-mode).
The useful pattern is restrained surfaces, compact controls, clear section
boundaries, and distinct object, camera, display, and selection roles.
[3ds Max's navigation documentation](https://help.autodesk.com/cloudhelp/2026/ENU/3DSMax-Basics/files/GUID-010B3CF4-0D7E-4886-8C65-BF332313D423.htm)
also groups navigation and orientation controls separately from model editing.

| Area | Change in Spice CAD |
| --- | --- |
| Application identity | Restore Spice CAD beside its mark; File, Edit, View, Model, Help remain global. |
| Viewport | Replace the floating, wrapping card with a fixed header: Assembly/Print, Frame, View, Display. Camera views include isometric, front, right, and top. |
| Properties | Clear title and part count; selectable, color-marked part rows; left-aligned parameter labels and wider sliders; readable measurement name/value rows; collapsible geometry statistics. |
| Feedback | A neutral Run button becomes available for changed source or pending parameter drafts, and disables while busy or synchronized. Live preview stays on by default. |
| Footer | Diagnostics, details, and build messages on the left; mode, line, column, and total lines on the right. |
| Settings menus | Toggles keep menus open. Outside clicks and Escape dismiss them; commands close explicitly. |

The editor remains the primary modeling surface. Plasticity's interactive view
cube and matcap palette are useful future work; this pass provides camera
orientations and existing edge/measurement settings.

## Holy Furries Studio

The following records the original product-specific design. Startup now uses
the shared baked animation from `holyfurries/brand-assets`, pinned at
`1de8f8f114ac73a0b0cc20e72edd36677c1cf89e`. Its full-window forest background,
transparent artwork, skip controls, and asset provenance are documented in
`native/assets/brand/README.md`. The custom product card remains in About;
it is no longer the startup splash.

The startup splash and About card trace the original fox-and-halo mark from
[static/mark.svg](https://github.com/holyfurries/website/blob/e16f05643eee14cc7b772dacfd6e9b6dfa38127b/static/mark.svg),
with the site's paper, red, ink, and forest palette from
[site.css](https://github.com/holyfurries/website/blob/e16f05643eee14cc7b772dacfd6e9b6dfa38127b/static/site.css).
The mark is drawn as bounded vector geometry in `native/src/studio.rs`; no
images, extra fonts, packages, or network requests are needed. Branding retains
the website's [asset terms](https://github.com/holyfurries/website/blob/e16f05643eee14cc7b772dacfd6e9b6dfa38127b/ASSETS.md),
which distinguish studio identity from MIT-licensed website code.

Startup branding covers the entire application window with the studio paper
background until a model is ready, with no minimum display time or timer. The
workspace continues preparing behind the modal splash so viewport bounds are
established and the first model fits correctly. Failed builds reveal the editor
and diagnostics instead of trapping the user behind loading. Help → About Spice
CAD reopens a compact identity card.

Spice CAD has its own tall geometric spice jar with chamfered shoulders, replacing
the generic wire cube. It appears in the header, splash, and About, with a reusable
vector at `native/assets/spice-cad.svg`, installed as the Nix desktop launcher
icon. The Holy Furries fox remains the studio
signature. Both marks render natively without an image decoder.
The jar has an empty interior and leans 45 degrees to fill its square icon bounds
while retaining the proportions of a tall spice jar.

## Verification

Regression checks exercise persistent toggles, outside-click/Escape dismissal,
menu switching, and stable hovered submenus over sixteen frames. Parameter tests
cover pending drafts before and after source edits. Native X11 checks confirmed
manual Run gating, automatic preview, and settings staying open after changing
Properties visibility. The private test document was restored byte-for-byte.
All 160 workspace tests passed. Clippy passed with warnings denied, rustfmt and
nixfmt passed, and the release application built successfully. Final native
captures show the full-window splash, About, all four parts and nine parameter
controls, part selection, and the minimum 1100 × 650 layout. Invalid and empty
document checks returned to the editor rather than remaining behind the splash.
The Nix package derivation evaluated successfully with the installed SVG icon.

Persistent hover flashing was not reproduced on X11. Baseline File → Export and
View → Appearance crops had identical pixels across twelve idle-hover captures
each. This does not establish the behavior of other dropdowns or compositors;
the affected dropdown still needs identification if flashing continues.

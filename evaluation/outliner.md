# Native part outliner

Parts in Properties now provide visibility checkboxes, selection, and a row/right-click
menu with isolation, framing, and source navigation. Exit isolation restores the
previous visibility; editing a checkbox commits the isolated view as the new
visible set. Show all restores the whole assembly. Framing a hidden part reveals
it. F and the viewport Frame button frame the selected part, otherwise the visible
assembly, while preserving camera orientation.
Framing the currently isolated part preserves isolation.

## Identity and lifecycle

Visibility and part selection use the existing unique `cad.show` names, rather
than output indices or geometry hashes. They survive geometry changes, output
reordering, and Assembly/Print switching. Added names start visible; removed names
are discarded. Renaming creates a new identity. Opening another document resets
the outliner. Failed builds retain the last valid model and its visibility.

Reveal source targets the stored `cad.show` call site only when its result revision
matches the current buffer. It moves the caret without editing source. This may
be a shared declaration inside a loop. Stale results disable the menu command.

## Rendering and inspection

The view sends one 32-bit part mask, encoded as two exact 16-bit values in the
existing GPU uniform. Hidden triangle and edge vertices are clipped before
rasterization. CPU face and edge picking apply the same mask, including occlusion.
Hiding a selected part clears its selection and dimensions; measurements involving
hidden parts disappear. Overall dimensions and framing use visible bounds.

Geometry, placements, model checks, thumbnails, and exports remain authored-model
data. Visibility does not rebuild geometry or change export contents. Computing
the visibility mask allocates no Rust heap memory. The existing rack-tools model
remains visible and frameable despite having no scripted output declarations.

## Validation

Tests cover identity reordering/removal, isolation restoration, visibility edits,
the highest mask bit, zero-allocation mask computation, named selection restoration
with shared geometry, stale source locations, hidden-part occlusion and edge picks,
visible bounds, and filtered dimension labels. Existing geometry, editor, cache,
export, and projection tests remain enabled.

Native X11/Vulkan captures in `output/native/ui-outliner/` verify checkbox hiding,
isolation, restoration, a real lip-height rebuild, Print layout, empty visibility,
part framing, and source navigation. With the four-part rack, hide/isolate kept
preview revision 1, one geometry upload, two geometry allocations, and one placement
upload (320 bytes); only uniform writes changed from one to three. The private
source fixture was restored byte-for-byte.

All 174 workspace tests passed, along with all-target/all-feature Clippy,
rustfmt, and the native development build. A final focused outliner run passed
after tightening isolation-preserving framing. A separate native 32-part fixture
isolated and framed part 31, confirming the highest mask bit in both triangle and
edge rendering; captures are in `output/native/ui-outliner-32/`.

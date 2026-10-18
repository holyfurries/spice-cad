# Model dimensions and named faces

Typed Luau face handles identify selectors on the final solid passed to
`cad.show`. `cad.face` accepts a unique name, an axis-aligned outward normal, and
an optional plane coordinate. `cad.measure` consumes two parallel face handles
and optional minimum/maximum distances. Names and measurements are bounded to
64 each. See `models/measured-block.luau` or Examples → Measured block.

The geometry worker groups tessellated triangles by their source B-rep face.
Every triangle in a candidate must agree with the selected planar normal and
plane within 0.001 mm. A selector must identify exactly one face; missing,
ambiguous, intermediate-solid and nonparallel references fail explicitly.
Selectors are reevaluated on rebuild instead of persisting numeric face IDs.
These are preview-mesh checks, not exact kernel metrology or a sketch solver.
Handles currently feed measurements rather than modeling operations.

Annotations resolve before accepting a preview and before creating export files.
Failed checks use the existing error path, preserving the last valid model and
disabling export for the invalid revision. Properties marks older measurements
and distinguishes reference dimensions from explicit checks. Ordinary code-defined
geometry does not imply that clearance or fabrication requirements were checked.

The presentation follows the [Plasticity Dimension demo](https://doc.plasticity.xyz/common/dimension):
small green values aligned with projected edges, subtle extension lines, and blue
active values on dark highlights. Near-facing bounding edges keep dimensions
around the model. Matching declared dimensions share the overall label; names
and check status stay in Properties and hover tooltips. Labels remain upright
and are read-only. Selected faces expose their names and a copyable selector.
Rendering uses cached bounds and resolved annotations; kernel work stays off the
UI thread. Face selection copies selectors only on explicit button activation.

Selecting an unnamed face also shows local dimensions, independently of the
Measures toggle. Planar faces use an orthonormal frame based on the longest
boundary edge; internal tessellation diagonals are excluded. Curved faces report
world-coordinate extents with explicit bounds labels. Per-face metadata is built
once with the geometry snapshot on the existing kernel worker. Selection only
reads the cached face and computes a small fixed-size reference measurement.

Shift-click pairs parallel planar faces; the first stays orange and the second
is blue. Their perpendicular separation appears on the model and in Properties.
Nonparallel/curved pairs preserve the prior selection, repeated picks toggle a
member out, and empty space clears the selection. Small or end-on spans use
extended leaders instead of being suppressed by projected pixel length. New
geometry clears selection; failed builds retain inspection of the last valid
model. Interactive inspection remains read-only and does not add constraints.

Validation: 76 workspace tests pass, Clippy passes with all targets/features and
warnings denied, and the native release build succeeds. Tests cover rebuilds,
ambiguity, missing faces, unsupported references, limits, failed export without
file creation, reference dimensions, rotated labels and duplicate suppression.
Native UI checks cover dimension display, stable layout after a failed check,
selection and copied references, visual replacement, and horizontal tab/space
alignment. Logs and screenshots are under `output/native/dimensions-*`.
Additional tests cover the actual rack's 8 mm inner lip and lip-to-tread distance,
rotated rectangle lengths, curved bounds, nearly parallel rejection, and pair
selection transitions. Inspection UI artifacts are under `output/native/inspection-*`.
Native checks verify the unannotated 8 mm lip with Measures off, 47 mm tread depth,
8 mm paired separation, rejected nonparallel picks, deselection, restored overall
dimensions, and byte-for-byte unchanged source.

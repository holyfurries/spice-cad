# Assembly collisions and four-view workspace

Verified 2026-10-21. Native SMLib performs on-demand assembly intersections after
bounding-box filtering. Results above 0.000001 mm³ appear as advisory overlaps;
zero-volume contact is allowed, and kernel failures remain explicit incomplete
pairs. Both solids are highlighted when selecting a finding. Separate print
placements are preserved and overlap does not block exporting individual parts.

Pair caches key immutable canonical bodies and assembly placements, retain only
weak body references and numeric results, and hold at most 496 entries for 32
parts. Stable pair order uses direct cache slots; reordered parts fall back to
identity matching. Failed checks are retried. Print placements do not invalidate
assembly results. No collision work runs during ordinary preview or navigation.

## Regression checks

Tests cover partial overlap, containment, identical solids, contact, disjoint
curved solids with intersecting bounds, assembly rotation, cache invalidation,
maximum part count, explicit failures, disabled gap sampling, and exported
reports. The default four-part rack checked six pairs without overlap or failure.
A separate two-part fixture reported the expected 600 mm³ overlap in CLI and UI;
repeating its UI check reported one cached pair and zero intersections.

## Performance evidence

A debug-build test with 32 separated boxes checked all 496 pairs in 3.016 ms
cold, with bounding boxes rejecting every intersection. Across 100 warm runs,
mean check time was 0.181 ms; every pair hit the cache and no intersections ran.
These are local measurements, not timing gates or complex-solid benchmarks.

Four views share triangle/edge buffers, part placements, and render pipelines.
Four independent 128-byte camera uniforms prevent the last prepared camera from
replacing the other views. Geometry remains canonical and is uploaded once.
Orthographic projection/picking tests verify exact axes, shared scale, linked
navigation, and perspective independence. Pan uses screen deltas rather than raw
mouse units, so pointer acceleration/device counts cannot amplify movement.

An isolated Xvfb session with Vulkan llvmpipe exercised perspective and all three
orthographic views, dimensions, navigation, resizing, collision selection, and
repeat checks. Geometry uploads stayed at one, geometry allocations at two, and
part-placement uploads at one. Resize UI CPU samples were 4.150 and 4.101 ms in
that debug session. This is a functional/upload check, not a hardware-GPU frame-rate
benchmark: four views process vertices four times while splitting the viewport's
pixel area. Redraws remain event driven.

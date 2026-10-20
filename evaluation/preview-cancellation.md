# Obsolete preview cancellation

Each submitted worker command carries a ticket from a shared atomic generation.
Actual source changes and document replacement advance that generation. Replacing
a document invalidates the old ticket even when both buffers have the same editor
revision number. Parameter drafts still coalesce until the worker is idle;
unapplied drafts do not cancel useful progressive updates during dragging.

Script previews check the ticket before compilation, between recipe evaluation
steps, before and after tessellation, and between presentation phases. A native
kernel call already in progress finishes before its next checkpoint. Completed
intermediate bodies remain in the existing 16-body / 16 MiB cache. Cancellation
does not publish a partial scene. Incomplete geometry preparation preserves the
previous finished-part cache; geometry completed before a later cancellation
can remain cached for reuse.

Cancellation produces a dedicated worker outcome rather than a geometry error.
Receiving it releases the worker's busy state. Dispatch consumes a pending
request; responses never consume requests. This also preserves a queued rebuild
when undo returns to the same editor revision as a cancelled in-flight build.
The latest source can then run normally. The last accepted scene remains visible
during this transition.

The cache's cancellation scope is cleared on both successful and failed builds.
Exports do not enter this scope and always perform their normal final-quality
validation. Worker shutdown invalidates preview tickets before joining; exports
already in progress finish normally.

Regression tests use deterministic checkpoint budgets to stop between CAD
operations and immediately before tessellation, then verify predecessor reuse,
latest-request recovery, scope cleanup, and export preparation. Worker tests
also cover obsolete invalid source, newer valid requests, and full-channel
shutdown. Cancellation does not interrupt individual SMLib calls or Luau's
existing bounded compilation/evaluation.

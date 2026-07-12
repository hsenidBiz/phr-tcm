# Runner desktop UX exit notes (v0.5.0, 2026-07-12)

Roadmap iteration 3 (renumbered to v0.5.0 after the UI reorg shipped as
v0.4.x).

## Delivered

- **Always-on-top runner window**: a second webview window (`runner`
  label, `#runner` hash route, 460x720, always-on-top) opened from Run
  Tests. Session handed off via localStorage; the main window stays usable
  behind it.
- **Step-by-step player**: per-step Pass/Fail, per-case comment, per-case
  timer feeding `duration_ms`, prev/next, and a Finish that resolves point
  ids and submits ONE run carrying overall outcomes + per-step
  iterationDetails + screenshots + associated bug ids.
- **Screenshots**: full-monitor capture via `xcap` (every monitor -> b64
  PNG) and clipboard image paste in the webview; attached to results.
- **Per-step results** use the REAL ADO step ids (`parse_step_ids` off the
  Steps XML - never index math) built into v1's actionPath-hex
  iterationDetails shape (golden-tested); additive/best-effort so a failure
  never loses the recorded outcome.
- **Bug filing**: from a failed case, BugDialog prefills title + repro from
  the steps; `file_bug` runs `detect_bug_type` (Bug/ReproSteps vs
  Issue/Description), creates the work item with Related links to the test
  case and PBI, uploads + links the screenshots, and the result gets the
  bug id in `associatedBugs`.
- Tests: 61 cargo + 33 vitest.

## Deferred / notes

- v1's region-select capture overlay is not ported; the runner captures
  whole monitors and the user attaches the relevant ones (simpler, and the
  compact window keeps itself out of the shot via always-on-top ordering).
- xcap capture and the WebviewWindow open are exercised manually (need a
  real display/second window) - unit tests cover the payload assembly,
  step-id/iteration logic, and bug flow via mockIPC.
- Result-attachment *viewing* (v1 loaded a case's prior screenshots) not
  ported yet - low value vs. cost; revisit if asked.

## Next (v0.6.0)

Work Manager depth: card detail drawer (assignee via members cache,
activity picklist, scheduling), comments with avatars, quick create, team
scoping, board filters.

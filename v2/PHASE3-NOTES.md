# Phase 3 exit notes — test runner core (2026-07-11)

Branch: `feat/tauri-rewrite`. Verdict: **GO** for Phase 4 planning, with the
runner's desktop-UX features explicitly deferred (below).

## Delivered

- **Test plan / suite / point / run APIs** in `ado_testplan.rs`, ported from
  v1 with wiremock coverage: paginated plan+suite listing via
  `x-ms-continuationtoken`, `find_requirement_suite` (stops paging on
  match), `ensure_requirement_suite` (area-matched reuse-first, skips
  permission-blocked plans, creates plan+suite only when none exist —
  covered by a "no POSTs on reuse" test), test points parse, manual run
  lifecycle (create run from points → map auto-created results → PATCH
  outcomes with Completed state + 1000-char comment cap → complete run).
- **Board visibility restored** (closes the Phase 2 deferral): submit_queue
  ensures the PBI's requirement suite best-effort before creating, using the
  PBI's real area/iteration path — same order and failure-tolerance as v1
  CreationWorker.
- **Run panel UI**: suite auto-resolve, point table with last-outcome
  colouring, per-point outcome select + comment, one-click "Record N
  outcomes" producing a completed ADO run with a web link.
- Tests now: 50 Rust + 10 Vitest, all green.

## Deferred runner-UX features (v1 has them; port when the desktop shell
work lands, they are additive)

- Screenshot capture + attach (`add_result_attachment` is implemented and
  tested on the Rust side; the xcap capture flow and attachment UI are not).
- Per-step iteration results (`update_result_steps` in v1) — additive.
- Always-on-top compact runner window, focus timer, bug filing from a
  failed step.
- Result pre-load of last comment (v1 get_result) and attachment viewing.

## Still pending

- Live end-to-end validation against real ADO (auth → browse → create →
  run) — user at the browser required; everything above is
  wiremock/mockIPC-verified only.

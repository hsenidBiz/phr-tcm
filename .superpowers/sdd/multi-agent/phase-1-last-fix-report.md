# Phase 1 last fix: report

Status: DONE (Minor 1 and Minor 2 of phase-1-recheck.md). Minor 3 and docs/backlog.html not touched.

## Minor 1: test files (src-tauri/src/test_files.rs, `commit`)
- Part file is now `.{name}.part-{pid}-{n}` (process-wide AtomicU64), still dot-led so `list` hides it.
- A module `write_lock()` (std Mutex, coordination not a cache, commit is synchronous) is held from the "already there" check through the rename.
- Tests (suite/test_files.rs, assistant_saves): 8 threads behind a Barrier, 4 MiB distinct contents each. Without replace: 1 Ok, 7 "already in Test files", file equals one content whole, no part file. With replace: 8 Ok, file one whole content, no part file.
- Without the fix (unfixed code, 3 runs): both tests failed 3 of 3 runs.

## Minor 2: flow save (src-tauri/src/ai_bridge.rs, src/api_templates/flow_store.rs)
- `flow_store::save_if(root, org, project, &flow, expect_absent)` returns `SaveIfError::Appeared` or `Failed(e)`. The load-check-write runs under a `flows_lock()` (std Mutex, sync only). `save` and `remove` take the same lock.
- The bridge computes `may_replace = replace && non-blank why` once, and writes with `save_if(.., !may_replace)`. On `Appeared` it answers 409 with the same sentence as the early check (shared `flow_already_exists`).
- Tests: store level (api_template_flows.rs) save_if absent/appeared/unreadable/plain-save, and 8 threads behind a Barrier give exactly 1 Ok and 7 Appeared; bridge level (ai_bridge.rs, api_template_routes) a flow written inside `open_db` (between the early check and the write) is not replaced for four bodies lacking replace-with-why (409, file unchanged), and is replaced (200) when the call said replace with a why.
- Without the fix (check in save_if scratch-disabled, reverted): 3 of 3 new failing tests failed (store x2, bridge x1; the replace-with-why bridge test correctly still passes).

## Runs
- `test_files::`: 68 passed (x3). `api_template_flows::`: 62 passed. `ai_bridge::api_template_routes::`: 33 passed.
- Full `cargo test --tests`: 5771 passed, 0 failed, 131 ignored (suite), plus bindings 4 passed.
- src/bindings.ts showed modified with no diff; `git checkout -- src/bindings.ts`.

## Choices where the brief was silent
- Lock covers the part-file write too (as briefed, "check through the rename"), so a 25 MiB save holds other test-file saves for its duration.
- `flow_store::save` and `remove` also take the flows lock, so the person's import and removal serialize with the check.
- The recheck refuses whenever the call lacked replace with a why and a file (even unreadable) exists, matching the early check.
- docs/backlog.html was already modified in the working tree by someone else; left out of the commit.

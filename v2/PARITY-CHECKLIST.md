# v1 → v2 Parity Checklist (v1.0.0 retirement sign-off)

> **1.0.0 update (2026-07-12):** the owner elected to close every DEFERRED
> item before 1.0.0 rather than sign them off as deferred. All nine are now
> SHIPPED (HTML export, tag autocomplete, manual module/preconditions,
> mid-batch cancel, run filters, uploaded-screenshot viewing, last-comment
> preload, suite->Edit/Run handoff, start/finish dates). Remaining
> non-parity: the two DROPPED items (focus timer, custom window chrome -
> both with replacements) and the four CHANGED shapes noted below.
> Migration and the branch merge are deliberately HELD: v2 continues
> development on feat/tauri-rewrite; v1 users are not being pointed over yet.

Compiled 2026-07-12 against v1 = v3.2.2 (PyQt5) and v2 = v0.7.0 (Tauri).
Statuses: **SHIPPED** (parity), **IMPROVED** (parity plus), **CHANGED**
(same job, different shape - review), **DEFERRED** (not built, reason +
trigger recorded), **DROPPED** (deliberately not porting, reason).

Every DEFERRED/DROPPED line is a veto point: flag any you actually need
and it goes back on the board before retirement.

## Authentication
| v1 capability | Status | Notes |
|---|---|---|
| Microsoft sign-in, Azure CLI public client, no PATs | SHIPPED | PKCE in Rust; localhost loopback (fixed AADSTS50011) |
| Token in memory only, never persisted | IMPROVED | Rust-only; cannot cross IPC (guard-tested) |
| Silent refresh near expiry | SHIPPED | refresh_token grant, 5-min early renew |
| Mid-batch interactive re-auth, retry in-flight item | CHANGED | v2: item fails + stays queued; re-sign-in via button, then retry the queue. Same no-data-loss outcome, one extra click |

## Configuration / scope
| v1 capability | Status | Notes |
|---|---|---|
| Org discovery, project list, PBI-only search | SHIPPED | Context bar, global scope |
| Module/Preconditions field mapping + name auto-pick | SHIPPED | Settings, per-project, skip fallback |
| Area/iteration selection for created cases | IMPROVED | Defaults to the PBI's own paths; per-batch override in review |
| Recently-used PBIs | SHIPPED | Last 8 per project |
| Light/dark theme | SHIPPED | + system-follow; new slate/green identity |

## Test case creation (Manual Entry / Import / Review / Progress)
| v1 capability | Status | Notes |
|---|---|---|
| Manual entry: title/steps/tags/automation status | CHANGED | Steps entered one-per-line ("action => expected") instead of a grid; the full grid editor exists on Edit Test Cases |
| Manual entry: module/preconditions inputs | DEFERRED | Import + Edit carry them; add two inputs on request (small) |
| Tag autocomplete from project tags | DEFERRED | Tags free-text; add on request (get_tags port is small) |
| xlsx/csv import, 9-column, row-numbered warnings | SHIPPED | Golden-tested vs v1 pytest suite |
| AI round-trip JSON import/export | SHIPPED | Exact v1 wrapper, golden round-trip |
| Template generation | SHIPPED | Same example rows |
| TestCaseID = UPDATE contract; title match warns only | SHIPPED | Both rules tested |
| One shared queue across tabs; draft persists | IMPROVED | Per-PBI drafts survive restarts |
| Review: validity + duplicate warnings before create | SHIPPED | Client mirror of is_valid blocks; duplicates warn |
| Serial create loop, suite ensure, link to PBI | SHIPPED | 500ms pacing, best-effort ensure, per-item isolation |
| Live progress during create | SHIPPED | Typed SubmitProgress events |
| Abort mid-batch | DEFERRED | Failed/unprocessed items stay queued; explicit cancel button on request |
| HTML report export | DEFERRED | xlsx + JSON shipped; HTML on request |

## Edit Test Cases
| v1 capability | Status | Notes |
|---|---|---|
| Fetch PBI's cases with full fields | SHIPPED | Steps parsed with REAL ids |
| Steps grid edit, add/remove/reorder | SHIPPED | Reorder via buttons (dnd-kit optional later) |
| Save with blank-never-wipes rule | SHIPPED | Tested |
| Export selected to xlsx/JSON | SHIPPED | |

## Run Tests / Test Runner
| v1 capability | Status | Notes |
|---|---|---|
| Points table with last-outcome colours | SHIPPED | |
| Quick outcome + comment recording | SHIPPED | |
| Result/date/text filters on the points list | DEFERRED | Small client-side add on request |
| Always-on-top compact runner window | SHIPPED | Second webview window |
| Step-by-step per-step Pass/Fail -> ADO step view | IMPROVED | Real step ids from XML (v1-faithful hex actionPath) |
| Screenshots: region-select overlay | CHANGED | Whole-monitor capture (xcap) + clipboard paste; region overlay dropped as the always-on-top window stays out of shot |
| Attach screenshots to results | SHIPPED | |
| View previously-uploaded screenshots | DEFERRED | Low value/cost ratio; on request |
| Preload last outcome | SHIPPED | From points |
| Preload last comment (get_result) | DEFERRED | Rust command exists; wire into runner on request |
| Timer -> durationInMs | SHIPPED | Per-case |
| File bug from failure (Bug/Issue process-aware) | SHIPPED | Related links to TC + PBI, screenshots attached, associatedBugs |

## Test Suites browser
| v1 capability | Status | Notes |
|---|---|---|
| Plan -> suite tree, suite-less plans hidden | SHIPPED | Same rule, server-side |
| Suite contents view | SHIPPED | Points read-only in place |
| Open a suite's cases into Edit/Run tabs | DEFERRED | In-place view covers browsing; cross-tab handoff on request |

## Work Manager
| v1 capability | Status | Notes |
|---|---|---|
| Columns from state categories; Later -> Done; heuristics | SHIPPED | All rules golden-tested |
| Drag-drop transitions; exact-column-name state preference | SHIPPED | Optimistic with rollback |
| Detail editor (title/state/assignee/activity/estimates/description) | SHIPPED | Dirty-fields-only PATCH; ADO 4xx verbatim |
| Start/Finish date editing | DEFERRED | Displayed; date inputs on request |
| Comments with avatars + posting | SHIPPED | v1 fallback chain incl. Graph base64 variant |
| Quick create Task/Bug assigned to me | SHIPPED | |
| Team scoping (team field values, UNDER clauses) | SHIPPED | Golden-tested WIQL |
| Board filters | IMPROVED | v1 had scope only; v2 adds text/type filters |
| Focus timer on work items | DROPPED | The runner's timer covers test execution - the actual use; veto to revive |
| Type icon/colour card badges | SHIPPED | |

## Platform / chrome / distribution
| v1 capability | Status | Notes |
|---|---|---|
| Frameless custom title bar, Win11 rounded corners, per-mode chrome | DROPPED | Native chrome; the web UI carries the identity. Veto to revive via decorum |
| High-DPI rendering | SHIPPED | Native to WebView2 |
| Keyboard: Ctrl+Shift+M mode switch | SHIPPED | + Ctrl+1..5, Ctrl+K |
| Team-members cache (24h) | SHIPPED | Query staleTime |
| Auto-update | IMPROVED | git-pull -> Velopack delta updates, ~10MB app vs ~300MB |
| No DELETE calls anywhere | SHIPPED | Guard test |
| Blank imported fields never wipe data | SHIPPED | Tested |
| Windows-first packaging + release pipeline | SHIPPED | release-v2.ps1, dedicated feed repo |
| CI (ruff+pytest on master) | CHANGED | v2 gates run locally in release-v2.ps1; GitHub Actions CI for v2 not set up - add on request |
| Automated tests | IMPROVED | v1: 94 pytest (logic only, zero GUI). v2: 66 cargo + 37 vitest (UI included) + E2E smoke on the packed exe |

## Summary
- SHIPPED/IMPROVED: 43 · CHANGED: 4 · DEFERRED: 9 · DROPPED: 2
- Every DEFERRED item has a concrete trigger ("on request") and most are
  small; the two DROPPED items have replacements or reduced relevance.

## Retirement steps remaining (need owner decisions)
1. Review this checklist - veto any CHANGED/DEFERRED/DROPPED line.
2. User migration: final v1 release (v3.2.3) whose update notes point at
   the v2 installer; v1 in-app messaging optional.
3. Archive: freeze v1 master (README pointer), archive or retire the old
   releases repo once users have moved.
4. Bump v2 to 1.0.0 and merge feat/tauri-rewrite to master.

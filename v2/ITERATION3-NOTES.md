# UI reorganization exit notes (v0.4.0, 2026-07-12)

User-directed restructure between roadmap iterations 2 and 3; the runner-UX
iteration shifts to v0.5.0 (roadmap renumbered accordingly).

## Delivered

- **Sidebar = the v1 tabs**, one screen each: Manual Entry, Import File,
  Edit Test Cases, Run Tests, Test Suites - reserved for test-case
  workflows so future tabs slot in.
- **Context bar owns the full scope**: org > project > **PBI picker**
  (v1 Config-screen parity - pick once, every screen scopes to it; selected
  PBI persists across launches). Right side: **Work Manager pill** (v1's
  spot), settings gear (Settings left the sidebar), account, theme toggle.
- **One shared queue** (v1 rule: every tab feeds the same queue): Manual
  Entry and Import File both write the per-PBI draft via useQueue; the
  shared QueueSection carries the review gate, live progress, exports.
- **Test Suites screen** (pulled forward from the polish iteration):
  plans -> suites tree via the new `list_plans_with_suites` (unreadable
  plans skipped, root suites stripped, suite-less plans hidden - the v1
  rule), requirement suites badged with their PBI, suite click shows its
  points read-only.
- Work mode: pill swaps the content to the board; clicking any sidebar tab
  returns to that tab.
- Browse.tsx and QueuePanel.tsx dissolved into PbiPicker / screens /
  QueueSection.
- Tests: 59 cargo + 30 vitest.

## Roadmap renumbering

v0.5.0 runner desktop UX -> v0.6.0 Work Manager depth -> v0.7.0 polish
(area/iteration pickers, recently-used, shortcuts, E2E) -> v1.0.0
retirement.

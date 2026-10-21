# tcm tool errors seen while writing Auto Run scripts, from 2026-10-21

Continues `tcm-tool-errors-2026-10-19.md` (rows 25-71). Collected while setting up and
scripting Strategic Objectives (PBI 134623) on QA Automation. "Fault" says whether the app,
the agent, the harness or the environment is to blame, and the fix suggested.

| # | tool | what was sent (short) | what came back (verbatim, short) | fault and suggested fix |
|---|------|-----------------------|----------------------------------|-------------------------|
| 72 | run_api_template, then reset_autorun_browser | a template run as evelyn1 after a replay left the Auto Run browser open; then the reset the answer names, with `release: true` | template: "the account evelyn1 was in use by the Auto Run browser" ... "close it with reset_autorun_browser and `release: true`"; reset without it: "Reset, but the sign-in has gone" (account still held); reset with it: "`release` is true or false"; release_autorun_browser: "is now `reset_autorun_browser` with `release` true" | App: the MCP schema of reset_autorun_browser lists no parameters, so `release` reaches the bridge as text and is refused, and the old release tool only points at it. Nothing in the tools frees the account. Freed it with `POST /autorun-release` on the bridge. Fix: declare `release` as a boolean in the tool's input schema (and accept "true"/"false"). |
| 73 | save_api_fixture | `skip_stages: ["Goal groups"]` for an OKR cycle (no Goal groups step) | "skip_stages: Goal groups is not a stage of Performance cycle wizard" | App: the stage's title is "Goal groups (done, or not part of this cycle)", so the short name the gate's own error used ("must perform Goal groups") is not accepted. `pms-performance-cycle/goal-groups` worked. Fix: match a stage by its title before " (", or name the id in the gate's error. |

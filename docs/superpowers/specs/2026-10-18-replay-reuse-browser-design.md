# Replays without re-signing in

Date: 2026-10-18. Status: design. Backlog item 27 (`docs/autorun/backlog-2026-10.md`, "Replays
without re-signing in", commit 7ef4465a).

## Intent

While healing a case, an assistant calls `release_autorun_browser` and then
`replay_autorun_to_step`, again and again. Each replay opens the browser and replays the whole
sign-in (login, password, the "Another active session" bootbox, the menu): 10-30 s each time,
many times per healing session. The goal is that a replay in a browser already signed in as the
right account goes straight to the case's area, and that releasing becomes a repair for a broken
browser, not a routine step.

## What exists (2026-10-18)

- **One supervised browser.** Discovery, the assistant's replay, the person's watched step and
  Try all share the one `Session` (`commands/autorun.rs`, `static SESSION`). There is no
  separate discovery or replay browser, so "discovery and replay share one browser" is already
  true of the process; what is not shared is the sign-in.
- **What closes it.** `replay_supervised` ends the assistant's discovery first
  (`end_discovery_for_replay`); `open_for_discovery` closes a browser the assistant's replay
  left open (`make_way_for_discovery`); `release_autorun_browser` closes the assistant's browser.
  The next user opens a new browser and signs in.
- **Why releases are routine.** The save guard (`no_save`) lives in the browser
  (`Cdp.armed`, per-tab guards, `Session.guarded_case`) and is cleared only by closing the
  `Session`: a discovery's `no_save` is one-way, and the assistant's replay never lifts a guard
  (`may_lift` is true only for the person). So the guide tells assistants to release between a
  no_save replay and a saving one.
- **Every replay signs in again.** `replay_to_traced` always calls `signin::sign_in_leased`,
  which clears cookies and restores the saved session or reruns the recipe. A replay that
  "carries on" (same case, later step, same page, under 10 minutes) skips all of that, but any
  other replay starts from the sign-in.

## Goals

1. **The guard belongs to the call that needs it.** A discovery's `no_save` lasts until that
   discovery ends; an action's or batch's `no_save` lasts for that call; a replay guards for the
   case it replays (its script's `no_save` mark) and lifts a guard an earlier call of the
   assistant set when its own case does not need one. Ending a discovery lifts its guard.
2. **A replay reuses a signed-in browser.** When the browser is alive, signed in as the script's
   account (the environment cannot change while a browser is open), the page is not refused
   (`signin::session_refused`) and the recipe's signed-in marker shows, a replay from the
   beginning skips the sign-in and goes to the case's area with `nav::reach_module(...,
   TripFrom::Elsewhere, ...)`, as `return_to_area` does. Otherwise it signs in as today. The
   preconditions and the case's setup run as today either way.
3. **Starting a discovery keeps the replay's browser.** `make_way_for_discovery` no longer
   closes a live browser the assistant's replay left: the discovery takes it over (its tabs other
   than the main one closed, the replay's stand forgotten). A replay after a discovery ends the
   discovery (its mapping summary kept) and keeps the browser.
4. **`reset_autorun_browser`.** A new MCP tool: closes the other tabs, answers or forgets any
   open dialog, lifts the assistant's guard, forgets the replay stand and the last page change,
   and loads the home page (`nav::load_home`), keeping the sign-in. Refused (409) while a replay
   or batch is running, and for the person's browser (as release is).
5. **Guide.** Release only when the browser is broken (frozen, wrong account, session gone, "the
   browser did not answer"); never between replays. A replay in a signed-in browser says so
   ("Signed in already: went to <area>.").

## Non-goals

- Changing unattended runs (`OneBrowser`), the recorder, template-held browsers or a case's
  setup browsers.
- Keeping a sign-in across app restarts beyond what the saved session already does.
- Reusing the person's watched browser for the assistant beyond what is allowed today.

## Anti-goals

- A save getting through for a case whose script is marked `no_save`, or during a discovery
  started with `no_save` - the guard must still be on whenever one of those is running.
- A replay acting as the wrong account, or on a refused session, to save time.
- The person's browser closed, reset or taken over by the assistant.

## Decisions (my calls - Avin to check)

- `start_autorun_discovery`'s `no_save` covers the whole discovery (until it ends), not only its
  first call: a discovery started "without saving" is one the person expects never to save.
- "Signed in already" is judged by `Session.account` (with `None` - after a setup made way - as
  not signed in), the lease's account, `session_refused` and the recipe's signed-in marker.
  A failed trip to the area falls back once to a full sign-in.
- A replay that finds a guard set for ANOTHER case's draft by an earlier call of the assistant
  lifts it (today it refuses with `guarding_other_case`); one set by the person is never lifted.

## Constraints

- No HTTP DELETE; no request of TCM's own to the application.
- Tests under `src-tauri/tests/suite/`. Several existing tests assert today's behaviour and
  wording (`autorun_held_browser.rs` release/close tests, `autorun_replay_to.rs` guard and stand
  tests, `autorun_discovery.rs` `BATCH_STOP` and no_save tests, `autorun_guide.rs:720-722`,
  `tcm_mcp.rs` tool lists): they change with the behaviour, never weakened to pass.
- A new MCP tool goes everywhere `release_autorun_browser` is: `mcp.rs`, the bridge routes,
  `ai_tools.rs` groups, `src/lib/mcpTools.ts`, the tool-list tests.

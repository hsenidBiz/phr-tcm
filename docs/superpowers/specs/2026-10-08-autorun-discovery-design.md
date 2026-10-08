# Auto Run: discover the live app before writing scripts

Date: 2026-10-08
Status: design approved in conversation (approach A, parts 1 to 3); this file is the written spec for review.

## Goal

Auto Run scripts are written from what the assistant saw in the running application, never from the application's source code. Before a script is written, the assistant explores the live app the way the Playwright test planner does (`PHR-PLAYWRIGHT-AUTOMATION/.claude/agents/playwright-test-planner.md`), and builds each step by carrying it out, the way the Playwright test generator does. The app enforces the rule at save time: a locator the app never saw on a live page is refused.

## Why this changes today's behaviour

Today the assistant is told to read the source for selectors:

- `src-tauri/src/mcp.rs` (~325): "You may read the application's source for SELECTORS".
- `src-tauri/src/autorun/guide.rs` (~3-5, 971-973, 1001-1049): "If you can read the application's source, USE IT to find selectors", a section on source-derived selectors, and "Where each fact is allowed to come from" naming the source for locators and navigation.

Its live tools only look at a browser the person opened (`get_autorun_page`, `probe_autorun_locator`, `try_autorun_action`, `replay_autorun_to_step`); it cannot open a browser, sign in or move around freely, and nothing it learns is kept.

## Owner decisions

| Question | Decision |
|---|---|
| Enforcement | The app checks: every locator in a saved script must have been seen on the live app |
| Sign-in during discovery | The assistant opens the browser and signs in by itself; the app replays the recorded sign-in and types the password, which the assistant never sees |
| Saving during discovery | Allowed; anything created uses the environment's test name prefix, deletes only records with that prefix (instructed and logged, not enforced) |
| Output | A saved map per area, reused by later scripts, re-explored when stale |
| Reaching a screen | The assistant finds it through the menus; if it cannot, it asks the person to record the area as today |
| Approach | A: a record of what was seen, checked at save time |

## Out of scope

- A Playwright-style markdown test plan, and turning discovery into new test cases.
- The app enforcing the test name prefix (it cannot tell a name field from a search box).
- Re-checking scripts already saved before they are next edited.
- Stopping the assistant from reading files. It runs in the app's repository; the save check makes code-derived locators useless unless they are also on the live page, which is the rule the owner wants.

## 1. Discovery sessions

New assistant tools (MCP + bridge routes, offered under the same gate as the other Auto Run tools: a dev build or Advanced Features on):

- `start_autorun_discovery { account }`: the app opens its own Auto Run browser, signs in as that saved account by replaying the recorded sign-in recipe (the password never crosses to the assistant), and returns the landing page's snapshot plus its address (path only).
- `discover_autorun_action { action }`: runs ONE action in the discovery browser: click, fill, select, press a key, go back, or navigate (held to the environment's allowed origins). Returns what happened: the new snapshot, an address change, a dialog or message that appeared, and the write requests (POST/PUT/PATCH/DELETE) the page sent.
- `get_autorun_page` and `probe_autorun_locator` also work on the discovery browser.
- `save_autorun_area { name, module, clicks }`: saves a screen the assistant found through the menus as an area. The app verifies it first: a fresh sign-in, then the clicks replayed, and it saves only if the clicks arrive. A refusal says what the page showed. Areas a person records are unchanged.
- `end_autorun_discovery`: closes the browser.

One Auto Run browser at a time. A discovery session counts as a run going: `start_autorun_discovery` is refused while a run, replay or supervised session holds the browser, and the Run buttons and the title-bar run pill treat discovery as a run going.

A failed or expired sign-in is reported as the run's sign-in reports it today; the assistant stops and reports, never retries in a loop.

## 2. The map

One map per project, next to the recorded areas: `<autorun root>/projects/<slug>-map.json` (written through the existing project file helpers, atomically).

Per area:

- `path`: how to get there (the area's clicks), `explored_at`, `account` (the account key, never a login).
- `pages`: per page seen, the address path (never the query string), the title, and every element seen: locator, role, accessible name, kind (button, field, link, table, dialog, other).
- `forms`: each field's label, type, and whether it is required.
- `outcomes`: what actions led to ("Save showed 'Saved successfully'", "Add opened the New leave dialog").
- `writes`: a log of every write request discovery sent: method, address path, time, and the step that caused it. No bodies, no query strings, no headers.

What adds to the map: every locator the app actually served or matched on a live page, from discovery, from the person's supervised browser while healing, and from replays. Each entry is attributed to the area the browser was in.

Stale: an area is stale when `explored_at` is older than 30 days, or a script failed in that area since it was explored. The guide tells the assistant to explore a stale area again before writing or repairing scripts there.

## 3. The save check

On `save_autorun_script`, every locator in the script must be in the map: every click, fill and check target, each link of a chained locator, the steps inside `when_visible`, and the tab actions. Pages counted: the script's own area plus the areas it visits or returns to. A `navigate` address must be one discovery has seen.

Matching: role + name compared case-insensitively with surrounding spaces trimmed; `text` and `css` locators exactly.

Three exceptions:

1. Text the script typed itself: a locator whose text or name contains a value the script filled in an earlier step.
2. Text from the test case: a check locator whose text appears in the case's own steps or expected results.
3. Unchanged steps of an existing script: a repair (a save with `edits`) checks only the steps it declares; the rest are exempt.

New scripts are checked in full. Saved scripts are checked only when next edited.

A refusal names the step, the locator, and says to find it on the live page first (`probe_autorun_locator` / `discover_autorun_action`).

The check sits beside the existing floor and repair gates in the save route, before anything is written.

## 4. What the assistant is told

- Remove every "read the source for selectors" instruction from the guide and the tool descriptions, including the section on source-derived selectors and the source line in "Where each fact is allowed to come from".
- Add: "Discover the live app; never read the application's code to write scripts. A save refuses any locator the app has not seen."
- A guide section listing areas with no map or a stale map.
- New command `/tcm:discover`: read the case; start discovery and sign in; find the area or ask the person to record it; explore the elements, forms and outcomes the case's steps need, trying each step live; write the script, test it with replay, save.
- `/tcm:heal` explores a stale area again before repairing. `/tcm:setup` mentions discovery.

## 5. In the app

A Discovery card in Auto Run setup, beside Areas and Quirks. Per explored area: when, which account, how many pages and elements, a Stale badge, the write requests discovery sent, and Forget map (clears the area's map so it is explored again; saved scripts keep running, only new saves need the area seen again).

The How To Use guide gets a Discovery section.

## 6. Errors and limits

- Browser busy: refused with the reason.
- Address outside the allowed sites: refused, as today.
- Must not save words: not applied to discovery (saving is allowed); every write is logged instead.
- An area whose clicks do not arrive: refused, with what the page showed.
- Advanced Features off: no discovery tools offered.
- No secrets, hosts or query strings in logs or the map.
- No HTTP DELETE is sent by the app itself; a DELETE the application under test sends from a click is the page's own request and is logged.

## 7. Testing

- Rust (`src-tauri/tests/suite/`): map store; matching rules; the save check (refusals, the three exceptions, repairs checking only declared steps, chained locators, navigate addresses); the area check by replaying clicks; the one-browser lock; a guard that "read the source" and "application's source" never return to the guide or tool descriptions.
- Tool tests for every new tool and its errors.
- Frontend: the Discovery card, Forget map, the Stale badge; discovery counted as a run going.
- Hand check owed against the real environment (sample data has no live app).

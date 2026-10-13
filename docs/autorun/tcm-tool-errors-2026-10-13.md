# tcm tool errors seen while writing and healing Auto Run scripts

Errors the `tcm` MCP tools returned to assistants from 2026-10-11 to 2026-10-13,
while about 50 scripts were written for PBI 135592 (Cycle Setup, Timeline,
Evaluators), three Timeline scripts were healed and a shared draft was rebuilt. Kept apart from
`agent-pitfalls.md` (what an assistant should do differently) and
`agent-pitfalls-2.md` (what the runner does): this is what each tool said, why,
and what in the tool would stop it happening again.

Each row says who it is on: **app** (the tool or runner behaves in a way that
is hard to know or work with), **agent** (a call that was wrong given the tool's
stated rules), or **environment** (not the app's doing). An assistant that
repeats an "agent" row has not read the tool's description; an "app" row will
keep catching the next assistant too.

## 1. The replay holds the browser, and nothing says so

The most common blocker, and the one that cost the most time.

| Tool | What came back | Cause |
|---|---|---|
| `start_autorun_discovery` | 409 `close the supervised browser first` | A `replay_autorun_to_step` had opened the supervised browser and left it open. |
| `save_autorun_component` | 409 `try the component live in discovery first` | Saving a component needs a live try, and discovery cannot start while a replay holds the browser. |
| `discover_autorun_action` | 409 `no discovery is going` | The first replay ends the discovery that was running. |
| `release_autorun_browser` | 409 `the person's Auto Run browser is open - ask them to close it, or to press Release Auto Run browser` | By design it never closes a browser that is open and answering. After a replay the assistant has no way to free it. |
| `run_api_template` | 502 `the account automation was in use by the Auto Run browser - try again when it is free` | The replay's browser stays signed in as the account, and PeoplesHR allows one session per user. |
| `start_autorun_discovery` | 409 `an unattended run is going - wait for it, or stop it first` | The person's own unattended run held the browser. This one is correct. |

**Who:** app. The replay answer does not say it leaves the browser open or that
discovery, component saves and template runs as that account are now refused.
Three separate assistants hit this independently; two of them spent several
tool calls retrying.

**What would help.**
1. Close the supervised browser when a replay that an assistant started
   finishes, or let `release_autorun_browser` close a browser that an assistant's
   own replay opened (never one the person opened).
2. If it stays open, say so in the replay's answer, with what is now blocked.
3. Let `run_api_template` and fixtures use a different account by default for
   the same machine, or name an available one in the refusal. Using `evelyn1`
   for templates worked and avoided the clash.

Working order for an assistant today: all discovery, probes and component saves
first, then scripts saved, then replays last.

## 2. Replay answers time out while the replay keeps running

| Tool | What came back |
|---|---|
| `replay_autorun_to_step` | The call times out on the client side; a second call answers 409 `a replay is already running`. The replay itself carries on and finishes. |

**Who:** app. The assistant cannot tell a hung replay from a slow one and has
to retry until the 409 stops. A replay of a script that walks the wizard takes
longer than the call's wait.

**What would help.** Return at once with a run id and let `get_autorun_failures`
or a status call report progress, or hold the call open for as long as the
replay takes.

## 3. The seen-check refuses locators that are correct

A save refuses any locator the app has not shown matching exactly one element.
That is the right rule for a click. It trips on these:

| Tool | What came back | Cause |
|---|---|---|
| `save_autorun_component` | `Action 2: main button[aria-label^="Edit "]:not(...) was never seen on the live app` | An `expect_hidden` on "every other Edit button". It is written to match nothing once the search filters the list; before filtering it matches several. It can never match exactly one. It was accepted after the selector had been probed once on the unfiltered list. |
| `save_autorun_script` | `#btnCookieClose was never seen on the live app` | The locator is in a saved component and is inside a `when_visible`, but the banner is not on the page when the script is tried. The agent worked around it with `visible: false`. |
| `save_autorun_script` (dry run) | `text "Reviewer:" never seen` | `expect_hidden` does not count as a sighting. The full name "Reviewer: 30%" had been seen. |
| `save_autorun_script` (dry run) | `text "%" #1 was seen but never counted as matching exactly one element` | An `nth` locator has to be probed exactly as written, with the `nth`. |
| `save_autorun_script` (dry run) | `progressbar "— Evaluators" never seen` | A partial name is not accepted at save even though the runner matches by "contains". Use the full name that was seen: "Step 4 of 9 — Evaluators". |

**Who:** mixed. The nth, partial-name and `expect_hidden` rows are each in the
guide once an assistant finds the right paragraph, but nothing in the refusal
says which one applies. The cookie-banner and "match nothing" rows are the app:
a `when_visible` or `expect_hidden` target is allowed to be absent, so the
check does not fit it.

**What would help.** Skip the seen-check for the targets of `expect_hidden` and
`when_visible`, or say in the refusal what the check wants for that action kind.

## 4. The probe cannot take a chain

| Tool | What came back | Cause |
|---|---|---|
| `probe_autorun_locator` | 503 / 400 `not a valid selector`, a querySelector SyntaxError | A list (dialog, then the button inside it) was passed. Scripts accept that form; the probe takes only a string or one object. |
| `discover_autorun_actions` | 400 `unknown field in` | A selector with an `in` key. The chain form is an array. (agent) |

**Who:** app for the probe; the second is the agent's. Because a chain cannot
be probed, a chain locator can only be recorded as seen by carrying out an
action on it, and a step that has to be probed in a state that is expensive to
reach is costly.

## 5. Mistakes in the call, given the tool's stated rules

| Tool | What came back | Cause |
|---|---|---|
| `save_autorun_script` | 400 `steps 1-4 and 6 changed but not declared` | Only the changed step was sent. A save wants every step of the script, plus an `edits` entry. |
| `save_autorun_script` | a repair refused after three (`open it in the app, save it there`) | Three repairs were spent on timing guesses before the cause was found (146318). |
| `discover_autorun_action` | `click ... matched 2` for button "Continue" | "Save & Continue" also matches; use `exact: true`. |
| `discover_autorun_action` | link "Performance Cycles" in `#ehrm-modulemenus-8` matched 2 | The module menu holds a second copy. Use `nth 0`, or the home page's Recently Accessed link. |
| `discover_autorun_actions` | `heading "Manage Performance Cycles" is not on the page (the page is /hr/home/index)` | Discovery lands on the home page; the area's path has to be walked first. |
| replay of 136805 | `i[class*="check"]` matched 2 | An off-screen duplicate icon at x = -166. Scope CSS with `main `. |
| `expect_attribute` | `no such attribute` (aria-pressed, aria-checked) on the evaluator toggles | The toggle button carries no state attribute. Nothing in the page can say on or off. |

**Who:** agent in each row.

## 6. The action vocabulary has gaps that assistants keep running into

| What was tried | What came back |
|---|---|
| `expect_enabled`, `absent: true`, `equals: null` | Not actions. There is no "is enabled" check, and `expect_attribute` cannot test a missing attribute. |
| `role switch` on the evaluator toggles | 0 matches when probed, although the accessibility tree lists a switch. |
| a click on a disabled button | fails after 15 s. The check has to be `expect_attribute disabled ""`. |
| a width, position or colour check | cannot be expressed; checked through a class name or the legend text instead. |
| `mark_autorun_suspected_defect` after a replay | 400 `no run on this machine has case 136601`. A replay does not count as a run, so a defect cannot be marked straight after one. |
| `get_autorun_failures` after a replay | Sees real runs only. |

**Who:** app. Assertions on position, colour and enabled state are in the case
wording of many Evaluators cases (136803, 136805-136808, 136809, 136844) and
were asserted by proxy.

## 7. Environment and size

| Tool | What came back | Cause |
|---|---|---|
| `db_query` | 502 `Named Pipes Provider: Could not open a connection to SQL Server [53]` | QA SQL Server unreachable for a few minutes (2026-10-11 and 2026-10-12). Came back on its own. |
| `get_autorun_guide` (no `quick`) | result of about 100,000 characters exceeds the maximum, saved to a file | By size. Several assistants read it in part from the file. `quick: true` is the right call, and a section argument would help. |
| any replay or `get_autorun_page` | about 4,000 tokens each | The whole accessibility tree comes back. A multi-script session pays for it every time. A `limit` or a "changed lines only" answer would help. |
| `save_autorun_script` with `dry_run: true` | answered `saved 1 script(s) ... (new)`, then the real save answered `(unchanged)` | The dry run appears to have written the script. Worth checking: a dry run that saves defeats its purpose. |
| `run_api_fixture` | warning `cycle 10376 does not start with the test prefix, so Clean up will not find it` | A fixture's `creates` entry takes the cycle id as its name. Clean up cannot find a fixture's cycle by prefix. |

## 8. Not a tool error, but found on the way

- **Shared drafts are opened by name.** 35 scripts open
  `AUTOTEST Annual Performance Review FY2026` and 3 open
  `AUTOTEST Evaluators weights 90 2027`. A bulk delete of all drafts on
  2026-10-11 removed both without a warning. Neither was in the record of
  test-made drafts, so Clean up would not have listed them either. Before
  deleting cycles, search the scripts and components for the names. See the
  project memory note `project_autorun_shared_cycle_drafts`.
- **A cycle list search then Edit is a race.** Typing in "Search cycles..." and
  clicking Edit straight away loses the click while the list redraws for the
  filter. Wait for the other Edit buttons to go and for the target's own to be
  visible. Components "Open shared draft Timeline step", "Edit newest annual
  review draft" and "Open shared draft Evaluators step" now do.
- **Wizard step counters change with the Eval Rules choices.** "Step 2 of 8"
  with only Goals or only Competencies on, "of 9" with both. Wait for
  "— Eval Rules", not the full name.

## Added 2026-10-13: errors from writing the Goal Groups scripts (PBI 135592)

| Tool | What was sent | What came back | Who |
|---|---|---|---|
| `save_autorun_script` | a locator checked earlier with `expect_visible` in discovery (`cell "Decrease value 100 Increase value %"`) | `never seen on the live app` | **app** - an `expect_visible` that passes in discovery records no sighting; only page reads (`get_autorun_page`, an action's page dump) do. A check the agent just watched pass is refused. |
| `get_autorun_guide` | no options | about 111k characters, saved to a file | **app** - too long to read in one go; every agent pays for it at the start. A short or sectioned guide (`quick`, or by topic) would help. |
| `discover_autorun_actions` | `return_to_area` with no saved script for the case | `case 0 has no saved script to name its area` | **app/agent** - discovery has the area it started in; it could use that instead of refusing. |
| `discover_autorun_actions` | 24 actions | `at most 20 actions in one call` | agent |
| `discover_autorun_actions` | `expect_attribute` name `checked` on a checkbox | `no checked attribute` | **app/agent** - the checked state is a property, not an attribute; the guide should say how to check a checkbox's state (or `expect_attribute` could read the `checked` property). |
| `discover_autorun_action` | a row-by-name chain for a row's Edit button | not found | agent - the grid's rows have empty accessible names |
| `replay_autorun_to_step` | after a discovery | ends the discovery and holds the browser | app (section 1); `release_autorun_browser` freed it every time |
| `run_api_fixture` | any fixture | `cycle ... does not start with the test prefix` | app (known): the fixture records the cycle under its id, not its name |

**What would help.** Record a sighting when an `expect_*` passes in discovery
(the element was found, so it was seen); offer the guide in sections; let
`return_to_area` in discovery use the discovery's own area; document (or
support) checkbox state checks.

## Added 2026-10-13: a waiting replay says too little

`replay_autorun_to_step` no longer times out on the assistant's side: each
call waits about 48 s and, while the replay is still going, answers

    {"status":"running","sentence":"still running, now at step 1 of 4 - call replay_autorun_to_step again with the same case (137605) and step (5) to keep waiting","step":1,"of":4}

The polling itself works. What is missing showed on case 137605: the replay
stood at "step 1 of 4" for over five minutes while step 1's actions timed
out one after another (a new failure picture every ~30 s, every one the
Definition Wizard on Rating Methods). The answers only ever said "step 1 of
4", so the assistant kept polling with no sign anything was wrong; meanwhile
`get_autorun_page` is refused with `a replay is going`, so it could not look.

| Tool | What came back | Who |
|---|---|---|
| `replay_autorun_to_step` (polled 4+ times) | `still running, now at step 1 of 4` each time | **app** - no hint the step is stuck on timeouts |
| `get_autorun_page` during the replay | `409: a replay is going - call replay_autorun_to_step ... to wait for it` | app (by design, but it leaves the assistant blind) |

**What would help.** Say in the "still running" answer which action the
replay is on (its kind and target), how long it has been on it, and how many
actions in this step have already failed or timed out; when the current step
has been failing for longer than its own timeouts allow, stop the replay and
answer with the failure instead of "still running". Optionally let
`get_autorun_page` (read-only) answer during a replay.

## Added 2026-10-13: "Must not save" does not stop every save (Goal Groups)

While scripting 137623-137625 and 137605, an assistant tried Distribute Evenly
with `try_autorun_action` for case 137596, a script marked `no_save: true`.
The click went through and the new weights **were saved** - Distribute Evenly
saves from the page straight away, and the must-not-save guard did not stop
it. It left the shared "AUTOTEST Goal Groups Look 2027" draft (cycle 10578) at
33/33 with a stale row; it was put back to 50/50 through the UI.

Related behaviour found the same day (PBI 135592, cycle Goal Groups step):

- Loading the Goal Groups step re-syncs its rows with the active Definition
  Wizard goal groups (adds new active ones, drops removed ones) and **writes
  them to the database on load** (`perf_cycle_goal_group`). A look-only script
  can therefore change its draft just by opening the step when the Definition
  Wizard's groups changed in between.
- Distribute Evenly saves immediately (one earlier agent reported its result
  was lost on Exit - the two reports disagree; the save-on-click one was seen
  on the database).

| Tool | What was sent | What came back | Who |
|---|---|---|---|
| `try_autorun_action` | click "Distribute Evenly" for no_save case 137596 | ok - and the weights were saved | **app** - the guard missed a save the page sends on click; it must stop every write a no_save case's page sends, or the try must refuse a no_save case's write |
| `discover_autorun_actions` | Continue right after the Proficiency Profile title appeared | click did nothing; next wait timed out (twice) | **app** - click swallowed while the step was still loading; fixed by waiting for "Manager (Saved)" |
| `save_autorun_script` | `[data-role="group-active-switch"] ... #2/#3`, `[data-role="group-status"] ... #3` used only in checks | never seen | **app** - a passing `expect_*` records no sighting (see above); clicking once fixed it |
| `save_autorun_script` | `main span[class*="-valid"]` | never seen, though probed earlier | **app** - the probe's sighting was lost when discovery ended |
| `save_autorun_script` (dry run) | `button "Edit" in div[data-cycle-id="{{setup.cycle_id}}"]` | seen but never counted as exactly one; the probe cannot take a chain | **app** |
| `start_autorun_discovery` after a replay/fixture run | 409 the browser did not answer on port 55079 | first release said nothing was held; next start worked | **app** - stale held browser |
| `get_autorun_failures` 137605 | 404 no run has case 137605 | | **app** - a replay is not a run, so its failure cannot be read or marked |
| `get_autorun_guide`, `list_api_fixtures` | full answers | over the size limit, saved to a file | **app** - too large |
| `db_query` with `statements` | 400 create, update and delete are switched off | | environment (setting) |
| `discover_autorun_actions` | a selector with both `css` and `text` | 400 one of role, text or css | agent |
| restore draft with `expect_hidden` inside `when_visible` `then` | 400 `then` cannot hold expect_hidden | | agent |
| `release_autorun_browser`, `end_autorun_discovery` while a replay ran | 409 a replay is going | | agent |

**What would help.** Make the must-not-save guard cover saves sent on click
(any write request from the page while a no_save case runs or is tried), or
have `try_autorun_action` refuse an action for a no_save case when it writes;
keep probe sightings after discovery ends; let a replay's failure be read and
marked like a run's.

## Added 2026-10-13: healing 137624 (Goal Groups, Definition Wizard restore)

137624's case checks all passed; the restore at the end of step 4 failed
because Rating Methods' Save & Continue, clicked the instant it appeared,
sent the browser to the PeoplesHR home page, so the restore never ran and the
Definition Wizard was left with 3 active goal groups (the look-only Goal
Groups cases then see 3 rows). Fixed by waiting for the Rating Methods list
(button "Expand 5-Point Scale") before the click - component "Open Definition
Wizard Goal Groups" v2. Not reproduced by hand, so a probable cause only.

| Tool | What was sent | What came back | Who |
|---|---|---|---|
| a run (unattended) | click `#btnRatingMethodsContinue-button` found the same instant | the page went to the home page; every later action timed out | **app** - a button present before its page has finished loading takes the click; a run could wait for the page to settle (no pending requests) before clicking |
| `record_autorun_quirk` | one new quirk | `400: 40 active quirks` - retire one first | **app limit** - with 40+ PBIs' worth of notes the cap is reached; consider a higher cap or per-area quirks |
| `try_autorun_action` | `use_component` during a replay | only the first action's detail, not a pass/fail for the whole component | **app** - a component try should answer each action, as `discover_autorun_action` does |
| `discover_autorun_actions` | click link "Home" (listed in the page tree) | `link "Home" not found` | **app** - the snapshot shows a name the click locator does not match |
| `discover_autorun_action` (draft) | `when_visible` whose `then` held `return_to_area` | `then cannot hold return_to_area` | agent (stated rule) - so a script cannot recover from a home-page bounce inside a guard |
| `replay_autorun_to_step` | replay to a step past the case's last | `the browser is on the page before step N` - reads like a stop partway | **app** - say the whole case was replayed |
| `list_api_fixtures` | no filter | about 58k characters | **app** - needs a filter or paging, like `list_api_templates` |

**Quirk housekeeping.** q2f0a34 (Timeline Combine toggle redraw; never
confirmed, only case 136601 is skipped) was retired to make room for the
Rating Methods note above.

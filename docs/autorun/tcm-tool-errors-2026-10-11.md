# tcm tool errors seen while writing and healing Auto Run scripts

Errors the `tcm` MCP tools returned to assistants on 2026-10-11 and 2026-10-12,
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

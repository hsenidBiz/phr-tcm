# Discovery speed: the application's refusals

Date: 2026-10-18. Status: design; Avin answered the open questions (2026-10-18). Backlog items 18 and 19
(`docs/autorun/backlog-2026-10.md`, "Discovery speed: validation refusals").

## Intent

While scripting PBI 135592 the assistants kept walking into the application's own validation.
A click "succeeds" (the button was clicked) while the application refuses it with a message,
and the assistant finds out several calls later, when a wait times out or the next locator is
missing. It then reads the page, works out the rule, and the next assistant hits the same rule
again.

**Goal:** the assistant learns that the application refused an action on the same call that
made it, with the application's own words, and an assistant starting on an area already knows
the rules earlier assistants met there.

## What Avin said (2026-10-18)

1. The refusal shows at once, typically as a toast at the top right, often with a failed
   network request whose body carries a trace id and an error message.
2. A refusal can also show in a different part of the page, even inside an iframe, and with
   no failed request at all (client-side validation).
3. Different modules show errors differently, so **the assistant judges** whether a message is
   a refusal. TCM does not decide error versus success from a page message.
4. A refusal the test case does not expect means the case is most likely wrong, or its
   preconditions are not met: the assistant surfaces that to the user rather than working
   around it.
5. The trace id is a unique identification of that one error, used to look up the server
   logs. It may or may not be used.
6. Discovery should find the rules; unattended runs may remember them too where it matters,
   since the assistant reads failed runs when checking or healing.

## Non-goals

- Deciding by itself that a page message is an error. Only a failed request is treated as a
  refusal without the assistant's judgment.
- Exporting the rules to the Playwright repo (the hand-over design already excludes
  validation rules).
- Changing what a script can check (`expect_visible {role: "alert"}` and friends stay as they
  are).
- A screen in the app for the rules. The guide is where they are read (Avin's answer 4 did
  not ask for one; it can come later).

## Anti-goals (a failure even if it works)

- A batch that stops on every success toast ("Saved"), making discovery slower than today.
- A trace id, a response body or a secret written into the rules file or a log.
- A message from before the click reported as if the click caused it.
- A new HTTP DELETE, or any request TCM itself sends to the application: TCM only listens.

## Part 1: what the page said after an action (backlog 18)

### When

After every action that can change the page - click, double click, key press, select, fill
followed by Enter, drag - in `try_autorun_action`, `discover_autorun_action(s)`, replays and
unattended runs. The shared hook is `runner::run_expanded`, after the action and its existing
post-action checks (blocked save, unexpected dialog, refused session).

### Signal A: new page messages

TCM reads the visible text of message-like elements before the action and again after it, and
reports what is new. "Message-like" means, in the page and in every frame TCM can reach (the
same frames `frame_step` reaches today; same-origin iframes at least):

- `[role=alert]`, `[role=status]`, `[aria-live]` regions (what `STATUS_TEXT_JS` reads today);
- elements whose class names contain one of: `toast`, `alert`, `notif`, `snackbar`, `swal`,
  `bootbox`, `invalid`, `error`, `feedback`, `validation`, `message`;
- the text an `aria-invalid="true"` field points to with `aria-describedby`, or the message
  element right after it.

"New" means text not present before the action, compared after whitespace is collapsed. Each
message is reported once, at most 300 characters, with where it was found (the page, or the
frame's title/name) - at most 5 messages per action.

Timing: the toast appears immediately and fades within seconds, so the "after" read happens
when the action's existing settle ends, and once more after a short wait (builder's call,
around 500 ms) if nothing new was found, before the next action runs.

A page message **does not fail the action**. It is reported on the action's result as
`page_said: [{text, where}]`, and in the human-readable detail as a line:

> the page showed: "At least one assessment component must be enabled." (top of the page)

If a **later action in the same batch fails**, the batch's stop line points back to it:

> stopped at action 4 (expect_visible "Goals"); after action 3 (click "Save & Continue") the
> page showed: "At least one assessment component must be enabled."

The guide tells the assistant (see Part 3) to judge each message against the test case: a
success message is ignored; a refusal the case expects is checked with an `expect_visible`
step; a refusal the case does not expect is surfaced to the user as "the test case or its
preconditions look wrong", with the message.

### Signal B: a failed request from the action

TCM already records every request per tab (`NetRecord`, `net_mark`/`net_since`). For the
requests the action started (those recorded after the action's mark), one counts when all
of these hold (my call, after the builder check):

- its resource type is XHR or Fetch (`NetRecord` gains the type from `requestWillBeSent`);
- it went to the same site as the page (`NetRecord` gains the host; it is compared, never
  shown or stored);
- it is not one of the recorder's polls;
- its status is 400-499 except 401, 403 and 404: 401/403 are the refused-session check's
  business, and 404 is too often an optional resource. **5xx stays with the existing page
  errors** (`page_errors`, `ignore_page_errors`, suspected defects), which keep judging it as
  the script asks - Signal B never fails or double-counts a 5xx. The one addition there: when
  a 5xx body is readable, its trace id is added to the existing page-error line, so the
  server log can be found.

Such a request is read for its body
(`Network.getResponseBody`, the same 64 KB cap and redaction `api_checks` uses). From a JSON
body TCM takes:

- the message: the first non-empty of `message`, `Message`, `error_description`, `error`
  (string, or its `message`), `detail`, `title`, then the first entry of `errors` (an object
  of field to list, or a list);
- the trace id: the first of `traceId`, `TraceId`, `trace_id`, `traceIdentifier`, or the
  `traceparent`/`x-correlation-id`-style value if the body has none (builder: response
  headers only if `NetRecord` already has them cheaply; otherwise body only).

A body that is not JSON gives its first 200 characters of text as the message, if it is not
HTML. A request whose body cannot be read (gone, too big) still counts, with its status as
the message ("400 Bad Request").

In discovery and tries, a failed request **fails the action** and stops the batch, like a
missing locator. A case that provokes the refusal on purpose costs one more call: the
assistant reads the message and sends the `expect_visible` that checks it in the next batch.
The script format does not change.

> the app refused: "Assessment weightage must total 100." (POST /api/eval-rules, 400, trace id
> 00-4bf92f3577b34da6a3ce929d0e0e4736-00)

The page messages, if any, are reported with it.

Wording: the existing `PAGE_REFUSED` ("the page refused: ") means a browser protocol error,
not the application. The new failure says "the app refused:" and is its own `ErrorClass`
(`AppRefused`, key `app_refused`, label "the application refused the action"), so the
failure patterns, suspected-defect counting and the healer tell the two apart.
(Builder's call: whether the old class is renamed in its label; its key stays.)

### Replays and unattended runs

The same two signals run after each action. A run step that fails names them in its failure
line, instead of only the later timeout:

> step 3: wait_for "Goals" timed out - after "Save & Continue" the page showed: "At least
> one assessment component must be enabled."

In replays and runs **neither signal fails a step by itself** (my call, after the builder
check): a saved case that checks a refusal on purpose - "weightage must total 100" - must keep
passing after this change. When the step fails for its own reason, its failure line names what
the page showed and any refused request, with the trace id ("the app refused: ... trace id
..."), and the failure is classed `AppRefused` when a refused request came before it. The
failures report (`get_autorun_failures`) and the run results carry the trace id with the step.

## Part 2: remembering the rules per area (backlog 19)

### What is remembered

A **rule** is `{page, after, message, source, case_id, first_seen, last_seen, count}`:

- `page`: where it happened - the page path as the discovery map writes it
  (`discovery_map::page_path`), plus the wizard step's name when the page has a step
  indicator (its active step's text), because a wizard keeps one address for every step;
- `after`: the action that caused it, in the short form batch lines use (`click "Save &
  Continue"`);
- `message`: the application's words as shown, record values included (no trace id, no body,
  no status). Avin chose this (2026-10-18, option A) over blanking quoted values and codes:
  it is the test environment's data. The discovery map's promise to hold no text from a
  record is loosened for `refusals` only, and its module notes say so;
- `source`: `discovery`, `run` or `assistant`.

Rules live in the area's entry of the discovery map (`projects/<slug>-map.json`,
`AreaMap.refusals`, serde default empty), deduplicated by `(page, message)` compared
case-insensitively after whitespace is collapsed; a repeat updates `last_seen`, `count` and
`after`. At most 30 per area; the one least recently seen goes. They survive a re-exploration
of the area (they are rules of the application, not of the map).

### When a rule is remembered

- **A failed request** (Signal B) in discovery or a try: always; in a replay or run, when the
  step then failed.
- **A page message followed by a failure** in the same batch or step: remembered, because the
  failure is the evidence it was a refusal.
- **A page message the assistant judges a refusal** without a failure following (for example
  the last action of a batch): the assistant records it with a new MCP tool
  `record_validation_rule {area?, page?, after, message}` (area defaults to the discovery's
  area, page to the current page). The same tool with `remove: true` removes a rule that no
  longer holds (the application changed).
- A page message alone is never remembered on TCM's own judgment.

Unattended runs and replays remember only from failed steps (Avin: "where required"), under
the area the run was in at that step: the script's `area`, or the area a component's
`return_to_area` had taken it to. A step with no known area is not remembered (my call: the
guide would never show the `""` bucket).

## Part 3: the guide

The quick live section gains `## Known validation rules`, after `## Areas to explore`: the
rules of the area being discovered (or, without a discovery, of every area with rules, at most
10 per area and 40 in all, most recently seen first), one line each:

> - Eval Rules, step "Assessment Components": after click "Save & Continue" -
>   "At least one assessment component must be enabled." (seen 3 times)

The full guide's discovery section explains, in a few sentences: what `page_said` and "the app
refused" mean; to judge each message against the test case; that a refusal the case does not
expect means the case or its preconditions look wrong and is reported to the user with the
message; to check an expected refusal with `expect_visible`; and when to call
`record_validation_rule`.

The How To Use guide gets a sentence where Auto Run discovery is described.

## Constraints

- All application I/O stays read-only listening through the existing CDP session; no request
  is sent to the application, no DELETE anywhere.
- The response body is read only for requests the action started with status 400 or above,
  never stored, and passed through `api_checks::shown_body`'s redaction before any of it is
  shown.
- The check adds at most one extra page read per action plus the short second read when
  nothing showed; it must not add a fixed delay to actions that changed nothing.
- Tests: Rust integration tests in `src-tauri/tests/suite/` (the `ScriptedDriver` in
  `common.rs` can script page reads and network events); MCP tool definitions regenerated as
  the existing tools are.

## Left to the builder

- The exact JS for the message read and how frames are walked (reuse what `frame_step` does).
- The second-read wait (around 500 ms) and whether it applies to keys and fills.
- Whether `page_said` appears in the JSON of passing actions in a batch only when non-empty
  (it should: no empty arrays in every result).
- How the wizard step's name is read (the active item of a step indicator), with the page path
  alone when there is none.
- The exact wording of guide lines, within the meaning above.

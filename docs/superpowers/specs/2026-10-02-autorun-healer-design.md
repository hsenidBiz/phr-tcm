# Auto Run healer: /tcm:heal and suspected defects

Design, agreed with the owner on 2026-10-02.

## 1. Why

The owner uses a Playwright "Test Healer" prompt elsewhere: run the failing
tests, pause on the error, inspect the page, find the root cause, fix one
thing at a time, retest, and mark a test `test.fixme()` with a comment when
the test is right and the application is wrong. Auto Run already has most of
the parts - `get_autorun_failures`, `get_autorun_page`,
`probe_autorun_locator`, `try_autorun_action`, the repair gate in
`save_autorun_script`, the three-repair cap, the STOP lines and quirks - but
they are spread through the guide, there is no single thing to invoke, and
there is no outcome for "the script is right, the application is wrong".
Today that case can only end in a repair, which is exactly the wrong
pressure: it pushes the assistant to make a correct script match a broken
application.

Success means:

- `/tcm:heal 501 502` walks an assistant through failing cases one at a
  time and ends with one line per case saying what was wrong and what was
  done.
- When the application does not do what the case expects, the case is
  marked as a suspected application defect, with a note on the step, and
  the script is left alone.
- Later runs label a failure at that step as a suspected defect, and the
  mark clears itself once the step passes.

## 2. Owner decisions

1. **A `/tcm:heal` command** and a **suspected-defect mark** - the two
   parts taken from the Playwright healer.
2. **Not taken:** re-running a case from the assistant to verify a repair
   (the person re-runs it), and a fully non-interactive healer (the person
   still opens the browser, signs in and brings it to the failing step).
3. **A marked case still runs**, labelled: a failure at the marked step
   says "suspected application defect"; a pass at that step clears the mark.
4. **The mark lives on the case's script**, set by a new tool. It is not a
   repair and never changes the script's actions.

## 3. The command: `/tcm:heal [case ids]`

A new entry in `ai_tools::COMMANDS`, stem `heal`, tool
`get_autorun_failures`. Because a command is left out whenever its tool is
disabled, it exists only where Auto Run is offered - no new gating.

Hint: `[case ids, or blank for every failed case in the newest run]`.

The body is the routine. It names tools, not code, and defers to the guide
for every rule:

1. Call `get_autorun_guide`, then `get_autorun_failures` for the given
   cases, or for every failed case in the newest run when none are given.
2. A case whose failure is one of the three `STOP:` lines is reported as it
   is and not touched.
3. Take one case at a time. Ask the person to bring the open Auto Run
   browser to the failing step; then look with `get_autorun_page` and
   `probe_autorun_locator`. Base the diagnosis on what the page shows, not
   on what the failure text suggests.
4. Name the cause as exactly one of:
   - the locator no longer matches (renamed, moved, duplicated);
   - timing (the element or answer arrives later than the step waits);
   - data or a missing precondition;
   - the environment (wrong site, wrong account, session);
   - the application does not do what the case expects.
5. For the first four: change only the locator, the waiting or the
   navigation; prove the changed action with `try_autorun_action`; save it
   through `save_autorun_script` with an `edits` entry and a `quirk` when
   the cause is about the application. Text that changes from run to run is
   matched on its stable part with a non-exact name. Fix one step, then
   look again before the next.
6. For the fifth: do not touch the script. Call
   `mark_autorun_suspected_defect`.
7. Never remove or weaken a check, never add a fixed wait, never exceed the
   repair cap; a refusal from the gate is final for that case.
8. Finish with one line per case: the cause, then what was changed (steps),
   marked (step and note), or why it was left; and ask the person to re-run
   those cases.

The guide's "Repairing a script that failed" section gains a short
subsection on the suspected-defect outcome (when to choose it, what it does
not do) so the rule holds without the command too.

## 4. The suspected-defect mark

### Data

`CaseScript` gains:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub suspected_defect: Option<SuspectedDefect>,

pub struct SuspectedDefect {
    pub step_number: i32,
    pub note: String,   // what the application did vs what the case expects
    pub marked_at: String, // epoch ms as a string, like LocalRun.started_at
}
```

One mark per case; a new mark replaces the old one.

### The tool

`mark_autorun_suspected_defect { case_id, step_number, note }`, an Auto Run
tool (dev-only / unlock-gated like the others). Refused when:

- the case has no script - `case N has no script to mark`;
- the step is not in the script - `step N is not in the script`;
- the step did not fail in the case's newest run - the same rule and
  sentence `record_autorun_quirk` uses for its `cases`;
- that run's failure for the case is one of the `STOP:` situations
  (sign-in failed, browser stopped answering, the person marked it
  Blocked) - `a STOP failure is not an application defect`;
- the note is blank, or longer than 300 characters.

The note is stored after the same address and token scrub API-check
excerpts use (a host and query are cut, bearer and JWT values redacted).
The tool never changes `steps`, `repairs` or `last_repair`, and does not
count toward the repair cap. It answers with the stored mark.

### Keeping and clearing it

- Saving from the editor keeps the mark (unlike `repairs`, which it
  resets).
- An assistant's save of the script keeps the mark, unless the save is a
  repair that changes the marked step - then the mark is removed: the
  assistant has decided the step was the script's fault after all.
- A person can clear it from the case row.
- A recorded run - unattended, or supervised once saved - clears it when
  the marked step passed in that run, using the same per-case pass in which
  quirks are counted. The run's case record says so (see below).

### In runs

When a case with a mark fails at the marked step, its proposal stays
`Failed` and its reason becomes
`Suspected application defect at step N: <note>`, followed by the usual
failure sentence. A failure at another step is a normal failure and the
mark stays. When the mark is cleared by a pass, the case record's reason
gains `The suspected defect at step N did not happen this time - the mark
was cleared.` The reason already travels into the Azure DevOps result
comment when a person presses Send (`publish::comment_for`); nothing else
is sent, and nothing is sent automatically.

### In the app

- **Case rows** (Auto Run test cases): a `Suspected defect` badge (warning
  token) beside the case, the step and note in its accessible description
  and tooltip, and a Clear button (`Clear suspected defect for #<id>`)
  with an inline confirm.
- **Review** and **Past runs**: the reason already shows; the run report
  (HTML) shows it in the failed-case section as it shows any reason.
- No change to the verdict buckets: a suspected defect is a Failed case.

## 5. Safety and scope

- Auto Run stays gated as today; nothing is named in the changelog, help
  site or README.
- The mark never changes a script's actions, never runs anything, and never
  reaches Azure DevOps except through a reason a person sends.
- No HTTP DELETE; removing a mark is a local file write.

## 6. Out of scope

- The assistant re-running a case to verify a repair.
- Creating Azure DevOps bugs from a mark.
- Pattern (regular-expression) locators; the non-exact name match covers
  dynamic text.
- A person setting a mark from the app.

## 7. Testing

- Command: `heal` is in `COMMANDS` with tool `get_autorun_failures`; it is
  left out when Auto Run is not offered; its body names every tool it uses
  and each tool exists; it contains no em dash.
- Tool: each refusal sentence; the stored mark; the note scrub; a new mark
  replaces the old one; `repairs` and `steps` unchanged; the tool is hidden
  and refused where Auto Run is not offered.
- Keeping and clearing: editor save keeps it; an assistant repair of
  another step keeps it; a repair of the marked step removes it; Clear
  removes it; a run that passes the marked step clears it and says so; a
  run that fails elsewhere keeps it.
- Proposals: failure at the marked step gives the defect reason; failure
  elsewhere does not; `comment_for` carries it.
- Guide: the new subsection exists and names the tool; the guide test's
  term checks still pass.
- Webview: badge, accessible description, Clear with confirm; the
  ui-consistency and a11y gates unchanged.

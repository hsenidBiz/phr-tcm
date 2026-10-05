# Auto Run: replay a case up to step N (part 4 of 5)

Design agreed with the owner on 2026-10-05, from item 6 of the backlog in
`docs/autorun/backlog-2026-10.md`. Healing needs the supervised browser at
the failing step, and today a person walks there by hand.

Owner decisions:
- Both the person and the assistant can start a replay.
- If no Auto Run browser is open when the assistant asks, the app opens one.
- On a script marked "must not save", an assistant's replay waits for the
  person to press Allow in the app.
- Ships as its own beta, after part 3.

## 1. What a replay does

`replay(case_id, step)` runs in the supervised Auto Run browser, in this
order:

1. **Checks before anything opens.**
   - The case has a saved script.
   - `step` is at least 1 and at most the script's last step number plus 1.
     The "plus 1" means "replay every step".
   - Refusals:
     - `case <id> has no saved script`
     - `step <n> is not in case <id>'s script (it has steps 1 to <last>)`
2. **The browser.**
   - An Auto Run browser that is already open is reused.
   - If none is open, one is opened: the browser last chosen in the
     unattended dialog, else Edge. This opening is the same as Open browser in
     the supervised pane, so the pane shows it as open.
3. **Preconditions,** exactly as a supervised case starts. While Database
   Read Access is off they are not checked, and the notice is carried. When
   one is not met, the replay stops with its Blocked sentence and nothing is
   signed in.
4. **The no-save guard** is set for the case when the script is marked must
   not save, through the same `guard_for_case` path as `auto_run_step`.
5. **Sign-in** as the case's account, through the supervised browser's
   lease. A held account refuses at once with the lease sentence.
6. **The trip to the case's area,** the same way a supervised run's first
   step does.
7. **Steps 1 to N-1,** run through `run_step_routed`, with the case's own
   timing and the supervised lease. After each step the pane hears progress
   as `replaying step K of N-1`.
8. **Stop.** The browser is left where it is, ready for step N. The answer
   is either:
   - `replayed case <id> to step <n> - the browser is on the page before step
     <n> runs`
   - or, when a step before N fails, `replay stopped at step <k>: <the step's
     failure sentence>`, with the browser left on step k.

   A failure outcome carries the step's screenshot, as a supervised step's
   does.

Only one replay runs at a time. A second request while one runs is refused
with `a replay is already running - wait for it to finish`.

A stop pressed in the supervised pane, or the browser closing, ends a replay
at once. The answer is then `the replay was stopped at step <k>`.

## 2. The person's button

- **The run review and Past runs:** a failed or blocked case whose failing
  step is known gets a `Replay to step <n>` button, with an accessible name
  of `Replay case <id> to step <n>`.
- **Pressing it** opens the supervised pane on that case, if it is not
  already showing, and starts the replay. Progress and the final sentence
  show in the pane.
- **The pane after a replay:** the steps that ran are marked as they would
  be in a supervised run. Step N is the next to run, so the person can carry
  on by hand.
- **Must not save:** a person's own replay of a must-not-save script needs
  no extra confirmation. The guard is on throughout.

## 3. The assistant's tool

A new MCP tool, `replay_autorun_to_step`, with `{ case_id, step }`. It is
offered wherever the Auto Run tools are offered (Enable Advanced Features,
or the extras switch, or a dev build), and it can be switched off on the
AI Bridge tab like the others.

- **The answer:** the same sentence as above, and on success the page
  snapshot (`get_autorun_page`'s shape), so the assistant can try step N
  straight away with `try_autorun_action`.
- **A must-not-save script:**
  - The app shows a modal: `The assistant wants to replay case <id> (<title>)
    up to step <n>. This script must not save; the guard stays on. Allow?`,
    with Allow and Deny.
  - Nothing opens, signs in or runs before Allow.
  - Deny gives `the person declined the replay`.
  - No answer within 2 minutes gives `the person did not answer within 2
    minutes`.
  - Only one request shows at a time. A second request while one waits is
    refused with `a replay request is already waiting for the person`.
- **The guide** (`autorun/guide.rs`, the healing section) says:
  - replay to the failing step before trying fixes;
  - the assistant may replay without asking, except that a must-not-save
    script asks the person in the app;
  - never replay past the failing step to "see what happens".

## 4. Safety

- **What a replay runs:** only the saved script's own steps, never edits.
  A repair is still saved through `save_autorun_script`.
- **Guards and limits:** the lease, the no-save guard, preconditions and the
  Database Read Access rule all apply exactly as in a supervised run.
- **The modal:** a must-not-save script is never replayed for the assistant
  without the person's Allow in that same session.
- **What it says:** no password, cookie, host, query string or SQL in any
  sentence, event or log line.

## 5. Out of scope

- Replaying in an unattended (headless) browser.
- Replaying several cases.
- Editing steps during a replay.

## 6. Testing

- Rust tests in `tests/suite` only, with the fake browsers:
  - the refusals;
  - steps 1 to N-1 run and step N does not;
  - stop at a failing step;
  - the stop control and the browser closing;
  - the lease and guard paths;
  - a precondition Blocked;
  - Database Read Access off;
  - one replay at a time;
  - the assistant's must-not-save request waiting, Allow, Deny, the
    2-minute timeout (as a parameter), and one request at a time;
  - the tool is offered with the Auto Run tools.
- **Live test (headless Edge):** a three-step fixture script replayed to
  step 3 leaves the page where step 2 left it.
- **vitest:** the button's presence and name, the pane's progress and final
  sentence, and the Allow modal.

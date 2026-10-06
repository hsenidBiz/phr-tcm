# Auto Run: several tabs

Design agreed with the owner on 2026-10-06 (backlog item 12 in
`docs/autorun/backlog-2026-10.md`).

Owner decision: scripts handle all three tab scenarios.

- **Follow a new tab:** a click opens a tab, and the script follows it.
- **Two tabs:** the script opens a second tab in the same signed-in session.
- **One tab closes:** the script closes a tab, or ends the session in one tab,
  then checks how the other tab reacts.

## 1. The model

- **Tab names.** A run's browser has named tabs. The tab the run starts in is
  `main`, and every step acts in the current tab.
- **Name rules.**
  - A name is 1 to 30 letters, digits, `-` or `_`.
  - `main` cannot be closed.
  - A step that names a tab that does not exist fails with
    `there is no tab <name>`.
- **One session.** Every tab belongs to the same browser and the same
  signed-in session. No tab signs in on its own.

## 2. New actions

- **`expect_tab { name, url_contains?, within_ms? }`**
  - Waits for a tab that the page opened since the previous step (a link with
    `target=_blank`, or `window.open`), and gives it `name`.
  - The default wait is 10 seconds.
  - If no tab appears, the step fails with
    `no new tab opened within <n> seconds`.
  - If the address does not match, it fails with
    `the new tab's address does not contain "<text>"`.
- **`open_tab { name, url }`**
  - Opens a new tab at `url` and switches to it.
  - The same origin rules and refusal sentences as `navigate` apply.
- **`switch_tab { name }`**
  - Makes `name` the current tab and brings it to the front.
- **`close_tab { name }`**
  - Closes the tab. If it was the current tab, `main` becomes the current
    tab.
- **`expect_tab_closed { name, within_ms? }`**
  - Passes when the page closes that tab itself, for example a print preview
    that closes after printing.

Each tab keeps its own page log, downloads and screenshots. A failure's
screenshot is taken of the current tab.

## 3. Rules

- **Other actions.** `click`, `fill`, `wait_for`, every check and
  `api_request` act in the current tab.
- **Every tab is guarded.** The must-not-save guard applies to every tab in
  the run, not only `main`.
- **No tabs carry over.** When a case ends, every tab except `main` is closed
  before the next case starts.
- **A session that ends in one tab.** To test this, use the existing
  `expire_session` in that tab, then `switch_tab` and a check in the other
  tab. No new action is needed.
- **A tab no step expects.** It is noted in the step's log as
  `a tab opened: <address without its query>` and left open. It never fails
  the case on its own.
- **Replay to step N.** It recreates the tabs the earlier steps made, because
  it runs those steps.
- **The supervised pane.** It names the current tab beside the step:
  `in tab <name>`.
- **The assistant's guide** explains the actions and the three scenarios,
  with one example each.

## 4. Out of scope

- A second signed-in user in another tab. A tab shares the session, so
  testing as two users stays a job for two cases with different accounts.
- Tabs in the API templates runner.

## 5. Testing

- **Rust** (in `tests/suite` only, with the fake browsers):
  - each action, and each of its failure sentences;
  - steps act in the current tab;
  - the must-not-save guard on a second tab;
  - tabs closed between cases;
  - a tab no step expects is logged;
  - replay recreates the tabs;
  - old scripts load unchanged.
- **Live test** (headless Edge): a local fixture page with a `target=_blank`
  link. The script follows the link, checks the new tab and returns to `main`.
- **vitest:** the supervised pane's `in tab <name>` label.

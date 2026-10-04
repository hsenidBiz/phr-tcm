# Auto Run and API Templates: quick wins (part 1 of 5)

Design agreed with the owner on 2026-10-05. It is part 1 of the owner's 2026-10-05 list of 11 suggestions plus 3 small fixes. The order agreed is:

1. quick wins;
2. run safety;
3. downloads;
4. healing reach;
5. fixtures.

Each part ships as its own beta. Auto Run and API Templates stay hidden, and nothing in this part is named in the changelog or help.

## 1. `save_autorun_script` keeps `edits` (small fix)

Owner report: "`save_autorun_script` through the tools drops the `edits` list, so every repair this week went through the bridge instead."

The MCP dispatch (`mcp.rs`, `save_autorun_script`) already tries to forward `edits`. The defect therefore lies in a payload shape the dispatch, or the route's body parsing, does not handle.

Reproduce it through the real dispatch with every shape an assistant sends:
- `scripts` as an array or as a JSON string, combined with `edits` as an array, as a JSON string, or as `null`;
- the whole bundle `{scripts, edits}` sent inside a `scripts` string;
- `edits` nested in each script.

Find the shape that loses `edits` and fix the root cause. When a repair arrives without `edits`, the refusal sentence must say that `edits` is missing.

## 2. `list_api_templates` can be read (small fix)

Owner report: "`list_api_templates` returned 210K characters, too big to read directly. It needs a filter or paging."

The tool takes optional arguments:
- `module`: case-insensitive substring match;
- `search`: a substring of the id, title, or a param/output name;
- `flow`: a flow id, which restricts the templates and flows to that flow;
- `offset` and `limit` on the template list (default 25, maximum 100).

With no arguments it returns a compact index: per template, the id, title, module, effect, proven state and stage, and per flow, the id, title and stage count. Full template detail (params, outputs, the newest run, the test files) is returned only when the filtered result holds at most `limit` templates. A new `id` argument returns that one template in full.

Every answer states its paging: the total, the offset, how many were returned, and the next offset if there is one. The tool description says how to page.

## 3. The deferred iframe minors (small fixes)

- The snapshot's iframe step is built from the frame's `id` or `title` (`browser/snapshot.rs`). Ids and titles must always produce valid CSS.
  - An id that is not a plain CSS identifier uses `iframe[id='<escaped>']`.
  - A title escapes `\` and `'`.
  - A test feeds odd values: a space, a quote, a leading digit, a colon, a backslash. For each, the printed step must parse as a selector and match its frame.
- "Cannot reach this frame" becomes its own failure class, `ErrorClass::FrameUnreachable`, in `autorun/patterns.rs`. Its key and label are `frame` / "a frame Auto Run cannot reach". It is about the application, so it can carry quirks, and `get_autorun_failures` groups it in patterns.

## 4. Dismiss-if-present steps in scripts (#1)

Scripts gain the recipe's `when_visible` step: `{ "kind": "when_visible", "selector": <locator>, "within_ms": <n>, "then": [actions] }`.
- If the target becomes visible within `within_ms` (default 2000, maximum 10000), the `then` actions run. Otherwise the step passes silently and the run moves on.
- `then` holds plain actions only: no `when_visible` inside it, no `sign_in`, and no checks (`expect_*`, `api_request`, `expect_response`). A guarded click is a tidy-up, not an assertion, so nothing inside it counts toward the expected-result floor.
- A `then` action that fails fails the step as usual.
- Outcomes: "not shown, skipped" when the target never appeared. When it did appear, the `then` actions' own outcomes.
- The guide documents it with the cookie banner and "Another active session" examples.
- The editor, the assistant's save, import and the try route all accept it. The recorder does not produce it.

## 5. The module path retries once (#4)

When an unattended or supervised run's trip to the module (`MODULE_STEP`) fails, the runner goes once more before it reports:
- it reloads the start address (the recipe's effective start URL), waits for the page to settle, and repeats the trip once;
- it logs the page's pending requests from the first attempt (the page log) to the application log, as `log_the_page` does today;
- a second failure is reported as today, Blocked with the reason, plus the sentence "tried twice, reloading the start page between".

The retry applies to every case, not only the first, because the stall is about the page and not about case order.

## 6. Transient failures retried once and labelled (#7)

After an unattended case fails, it is classed as transient when its first failure is one of these:
- an API check (`expect_response`, `api_request`), or the page's own request seen by the runner, that answered 502, 503 or 504, or 400 with an empty body;
- a network-level failure (`net::ERR_*`) on a request the step waited for;
- the browser stopped answering mid-step (the harness case), which already reports Blocked.

A transient case runs once more, from sign-in, in a fresh browser. This is controlled by a run option, "Retry transient failures once", on by default in the unattended dialog. Its choice is remembered like the browser choice.
- If the retry passes: the proposal is Passed, and the reason is "passed on a second try after a transient failure: <first failure sentence>".
- If it fails: the proposal is the second attempt's result, and the reason also names the first attempt.
- The run record keeps only the final attempt's steps, plus `retried: Some(<first failure sentence>)` on the case.
- Review, Past runs and the report show a small "Retried" label on such cases.

Only one retry is made, and only for these classes. An assertion that fails is never retried. Re-running only the failed cases is already served by the Last result filter shipped in 2.0.9-beta.11, so no new control is added for it.

## 7. Testing

- Rust tests go in `tests/suite` only. vitest covers the webview pieces: the dialog option and the Retried label.
- Each section's test names the exact sentences above.
- Run one test command at a time.

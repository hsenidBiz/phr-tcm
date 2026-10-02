# Auto Run API checks: expect_response and api_request

Design, agreed with the owner on 2026-10-02.

## 1. Why

Auto Run drives the application through Chrome DevTools, but a script can
only check what the screen shows. A save that the screen reports as done while
the API answered 500, or a record whose stored values differ from what the
screen says, passes unnoticed. The owner wants scripts to check the API too:

- **A - `expect_response`:** after a UI action, check the request the page
  itself made - that it happened, its status, and fields of its JSON answer.
- **B - `api_request`:** send a GET from the signed-in page and check the
  status and the JSON answer - for example, read a record back after the UI
  created it.

Success means: a script step "Click Save" followed by
`expect_response POST .../Save -> 200 {"success": true}` fails when the page's
save request answers 500 or `success: false`, and passes otherwise; and an
`api_request` GET of the saved record fails when a stored field differs from
the expected result - both with a sentence saying exactly what differed, and
neither leaving a cookie, token or header in the run file.

## 2. Owner decisions

1. **Both A and B.**
2. **B only reads.** GET only; setting data up stays with API templates.
3. **B's request is written in the step** (address, query, expected status and
   JSON) - no template reference.
4. **A looks at the requests made since the current script step started**,
   waiting up to a few seconds for a match.
5. **A checks status and JSON fields**, with the API templates' partial match.
6. **The assistant uses the proven API templates as reference** for real
   endpoints, paths and response fields when it writes these steps.

## 3. The steps

```json
{ "kind": "expect_response", "method": "POST", "url_contains": "/PerformanceCycle/Save",
  "status": 200, "json": { "success": true }, "timeout_ms": 10000 }

{ "kind": "api_request", "path": "/api/cycles/42", "query": { "include": "rules" },
  "expect": { "status": 200, "json": { "name": "Q4 Cycle" } } }
```

**`expect_response`**
- `url_contains` (required, non-empty): matched case-insensitively against the
  request's address path and query (never the host). `method` optional
  (any when absent). `status` default 200. `json` optional. `timeout_ms`
  optional, default the run's expect timing (10 s).
- Looks at requests the page started since the current script step began.
  Waits until a matching request has finished, up to the timeout. When
  several match, the most recent finished one is checked.
- With `json`, the response body is read (at most 64 KB) and must be JSON;
  the listed keys must match (nested objects recurse, arrays and scalars by
  equality, extra keys ignored) - the rule `api_templates::exec::partial_match`
  already implements.
- Failures, each its own sentence: no matching request within the time (naming
  the pattern and how many requests the step made); it failed at the network
  level; wrong status ("POST .../Save answered 500, expected 200"); not JSON;
  a field differs (naming the key, expected and actual). A failure adds an
  excerpt of the body (500 characters, tokens scrubbed, as API templates
  already do).

**`api_request`**
- GET only. `path` must pass `api_templates::is_safe_relative_path` (one
  leading `/`, no `..`, no backslash, no encoded tricks) - it is always the
  current page's own site. `query` is a map, percent-encoded as API templates
  encode it. `expect.status` default 200; `expect.json` optional, same partial
  match.
- Sent by the page itself (`fetch` with the page's cookies, no anti-forgery
  token, which GET does not need), so the screen is not changed. Timeout: the
  run's action timing.
- Failures as for `expect_response`: network failure / timeout, wrong status,
  not JSON, a field differs - with the scrubbed excerpt.

Both are **checks**: they satisfy "every expected result needs a check", and
the assertion-source rule holds - what they check comes from the case's
expected result, while the address and fields may come from the source or the
proven API templates.

## 4. How it works

- **Network record.** Every Auto Run browser - unattended, supervised and the
  assistant's single "try" step - has Chrome's network monitoring on from the
  moment it opens. A new record beside the existing page log
  (`browser/page_log.rs`, which keeps its run-log summary unchanged) keeps, per
  request: an id, method, address path + query, start order, status, content
  type, and whether it finished or failed. Bounded to the last 400 requests;
  never request headers, request bodies or cookies.
- **Step marker.** When a script step starts, the runner records the current
  position in that record; `expect_response` only looks after it.
- **Waiting.** Chrome's events are read during the app's DevTools calls, so
  `expect_response` polls with a light call until a match finishes or the time
  runs out; navigation and uploads must not clear the record.
- **Bodies** are fetched from Chrome only when a JSON check needs one
  (`Network.getResponseBody`); a body Chrome no longer holds is a clear
  failure ("the response body was no longer available").
- **`api_request`** uses a small in-page GET function in the style of the API
  templates' fetch, returning status, content type and text (capped); the
  shared pieces (safe path, query encoding, partial match, excerpt, token
  scrubbing) are reused, not copied.

## 5. Results, safety and the assistant

- Run files keep only the outcome sentence and the scrubbed excerpt - never
  headers, cookies or tokens. A failed check takes a screenshot as any failed
  check does.
- Failure-pattern grouping (quirks part A) gains an "API" class so repeated API
  failures across cases are grouped.
- The Auto Run guide documents both steps and their limits, and tells the
  assistant to look up real endpoints, paths and response fields in the proven
  API templates (`list_api_templates`) - as reference only.
- The editor, the assistant's save path, script import and the "try" step
  accept both kinds; validation refuses an empty `url_contains`, an unsafe
  path, a method other than GET for `api_request`, and a bad method name for
  `expect_response`.
- Auto Run stays gated as today; nothing is named in the changelog or help.

## 6. Out of scope

- Requests from embedded frames, workers or service workers.
- Checking request bodies or headers, or response headers.
- Write requests in `api_request`; reusing API templates in a script.
- Playwright: whether to drive or watch the Auto Run browser with Playwright is
  an open question for the owner, not part of this design.

## 7. Testing

- Pure logic: matching (method, pattern, case, "since the step started", most
  recent finished wins), the 400 cap, status/JSON checks and every failure
  sentence, body excerpt scrubbing, `api_request` validation (unsafe paths,
  methods), query encoding.
- Driver-level, with the suite's fake DevTools driver: a step marker plus
  emitted Network events -> pass; no event within the time -> the timeout
  sentence; a body fetch for the JSON check; a navigation in the same step does
  not clear the record; `api_request` calls the in-page function and checks
  its answer.
- Guards: no header, cookie or token text in a stored outcome; both kinds count
  as checks; the guide's action list matches the serde names.
- One real-browser test (ignored by default, like the existing ones) against a
  local page that issues a fetch: `expect_response` passes and fails as
  expected, and `api_request` reads a local JSON endpoint.

# The assistant's network, cookie and element tools

Date: 2026-10-19. Status: design. Backlog items 29 and 30 (`docs/autorun/backlog-2026-10.md`).
Notes from studying another tool: `docs/autorun/notes-network-capture.md`.

## Intent

Avin (2026-10-18): give the assistant more to go on while it discovers and heals - the page's
real requests and responses, its cookies, and what an element looks like and listens to - so it
can build **API templates** from real requests instead of guessing their shape, and **test edge
cases** (a changed header, a feature cookie, a style that must change). TCM's own versions,
behind TCM's guards; not a separate stealth browser (rejected: it would bypass the save guard,
the seen check and the session rules).

## Decisions (Avin, 2026-10-18)

- All 13 network and cookie tools (list, request details, response details, response content,
  search, export, import, set/get capture filters, modify headers, get/set/clear cookies), with
  guards; and read-only element inspection: computed styles (plus a script check
  `expect_style`), event listeners, animations/transitions, one element's structure. Element
  cloning and asset extraction are not taken.
- Purpose for the acting tools (headers, cookies, import): "make the assistant more capable to
  test edge cases and give it more tools to work with".

## Where it works (my calls)

The supervised Auto Run browser (`Session`): discovery, tries, the assistant's replays and
watched steps. Unattended runs are not captured (a run's browser is closed when it ends; the
assistant reads a failed run through its failure lines). Capture is always on while that browser
is open, within the budget below, and cleared when the browser closes or the account changes.

## Part 1: capture

For the page's own requests - XHR, Fetch and Document, to the same site, not the recorder's
polls (the same filter `refusals::counts` uses, without the status rule) - keep:

- method, path and query (the query shown, never stored outside memory), status, resource type,
  mime, sizes, start time and duration, redirect (first hop), failure reason;
- request headers and response headers from an allow-list (content-type, accept, content-length,
  x-requested-with, x-csrf-token *name only*, any `x-` header not matching the secret words);
  never `cookie`, `set-cookie`, `authorization`, `proxy-authorization`;
- the request body (`postData`, or `Network.getRequestPostData` when the event says there is one
  and omitted it), at most 256 KB;
- the response body, read at `loadingFinished` (later the browser may have dropped it), only for
  text-like types (json, text, javascript, xml, form-urlencoded, graphql, html), at most 2 MB
  each, within a 32 MB budget per browser, oldest bodies dropped first (the request stays);
- at most 1000 requests, oldest first.

Every body and header value the assistant sees passes through the existing redaction
(`api_checks::shown_body`, the secret words): passwords, tokens, session values are hidden.
Capture filters (resource types to include or exclude, a path substring) decide what is kept from
now on; what was filtered is not recoverable.

## Part 2: tools (MCP, beside the other Auto Run tools)

| Tool | Does |
|---|---|
| `list_autorun_requests` | newest first, paged: id, method, path, status, type, sizes, when |
| `search_autorun_requests` | substring on path, method, status, type, request body, response body; paged with total |
| `get_autorun_request` | one request: headers (allow-listed), body (redacted) |
| `get_autorun_response` | one response: status, headers (allow-listed), mime, size, timing |
| `get_autorun_response_body` | the body text (redacted), in pages when large |
| `set_autorun_capture` / `get_autorun_capture` | the capture filters |
| `export_autorun_requests` | writes the capture (redacted, as HAR 1.2) to TCM's own folder `<autorun root>/captures/<name>.har`; returns the file name |
| `import_autorun_requests` | loads one of those files as a separate read-only set the list/search/get tools can read with `from: "<name>"`; never replayed into the browser |
| `set_autorun_headers` | extra request headers for the page (`Network.setExtraHTTPHeaders`, one merged set per tab, applied to new tabs too); `null` removes one TCM set; lasts for the call or the discovery that set it (as the save guard does); refuses `cookie`, `authorization`, `proxy-authorization`, `host`; every answer while headers are set names them |
| `get_autorun_cookies` | name, domain, path, expiry, flags, size; the value hidden for the sign-in session's cookies and names matching the secret words |
| `set_autorun_cookie` | sets one cookie for the site; refuses the sign-in session's cookies (the set the cookie fingerprint covers) and secret-word names |
| `clear_autorun_cookies` | a real sign-out: clears the site's cookies, sets `Session.account` and the cookie fingerprint to none, tells the panes (the reset's event); the next replay signs in |
| `inspect_autorun_element` | for a locator (the same locator rules as a try): `styles` (computed values, all or the properties asked), `listeners` (`DOMDebugger.getEventListeners` on the element and on the element at its centre if another covers it), `animations` (running animations and transitions with durations), `structure` (tag, attributes, text, children summary, frame) - any of the four, default all |

`set_autorun_headers` does not use `Fetch`, so it never touches the save guard's interceptor
(`Fetch` and `setExtraHTTPHeaders` are independent); a blocked save stays blocked.

## Part 3: `expect_style`

A script check: `{"expect_style": {target, property, equals | contains | not_equals}}` - reads the
computed value (`getComputedStyle`) with the same wait and retry as the other `expect_` checks;
colours compared after normalising to `rgb()`/`rgba()`. The seen check treats its target like
any check's. Guide and the script editor list it.

## Anti-goals

- A secret (password, token, session cookie value, authorization) shown to the assistant,
  written to a file or a log.
- Any request TCM itself sends to the application, a DELETE, or a capture replayed into the
  browser.
- A save getting through the guard because of a header change; the account changing because of
  a cookie set.
- Unbounded memory: a long discovery must stay within the budgets.

## Constraints

- One CDP session per tab: capture hooks into the existing event routing (`cdp.rs`, beside
  `net_record.observe`), tab listener for new tabs.
- Raw CDP cookie commands (another tool found the typed parser hangs on a missing field).
- Tools registered everywhere the other Auto Run tools are (`mcp.rs`, bridge routes, `ai_tools.rs`
  groups and per-environment switches, `src/lib/mcpTools.ts`, the tool-list tests).
- Rust tests under `src-tauri/tests/suite/`; the guide documents each tool briefly and says to
  build API templates from captured requests.

## Left to the builder

- The exact allow-list beyond the named headers, HAR field mapping, page sizes for bodies.
- How `inspect_autorun_element` picks the covering element (the centre point TCM's click uses).

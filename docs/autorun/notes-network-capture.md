# Notes for backlog 29: network and cookie tools

Read 2026-10-18 from stealth-browser-mcp (MIT, Python/nodriver, `src/network_interceptor.py`,
`src/server.py`) to learn the approach; TCM writes its own on its CDP driver.

## Their approach, and what to keep

- Capture per tab: `Network.enable`; `requestWillBeSent`, `responseReceived`, `loadingFinished`.
  Bodies are read with `Network.getResponseBody` **in the `loadingFinished` handler** - later the
  browser may have evicted them.
- Body rule worth keeping: text-like only (xhr, fetch, document, script, eventsource, or a mime
  with json/text/javascript/xml/form-urlencoded/graphql); skip images, media, fonts, websockets;
  per-body cap 2 MB; per-browser byte budget 64 MB with oldest-first eviction (request metadata
  kept when its body is dropped); 1000 requests per instance.
- Search: case-insensitive substring on url, method, status, response body, request payload,
  resource type, ANDed, paged with `total` and `has_more`. Big answers spill to a file.
- Cookies via raw `Network.getCookies` / `getAllCookies` / `setCookie` / `deleteCookies` /
  `clearBrowserCookies` (their typed parser hung on a missing field in current Chrome).
- `modify_headers` = `Network.setExtraHTTPHeaders`: tab-wide, replace-all, no Fetch.

## What they do not handle (TCM should)

- Redirects (a second `requestWillBeSent` with `redirectResponse` overwrites the first; TCM's
  `NetRecord` already keeps the first hop).
- Large request bodies missing from the event: fetch them with `Network.getRequestPostData`.
- Failed, cancelled and never-finished requests (TCM already tracks `NetState::Failed`).
- Workers and out-of-process frames.
- Redaction: none at all (Authorization, cookies, set-cookie, bodies exported in plain text).
  TCM redacts with `api_checks::shown_body` and its secret words, and never shows session cookie
  values.
- Export/import paths are unrestricted; TCM keeps them in its own folder.
- Export format is their own JSON; HAR would be more useful to a person.

## Collision with TCM's save guard

The save guard uses `Fetch` interception. A second `Fetch.enable` on the same session replaces
the pattern set, and each paused request must be continued, fulfilled or failed exactly once. So
per-request header rewriting must go through the guard's own dispatcher (guard decides first). A
static header set can use `Network.setExtraHTTPHeaders` (one merged set per tab, replace-all),
which does not touch `Fetch` at all.

# Code-review findings: design

Date: 2026-09-18. Source: a five-reviewer read-only review of `main` at 1.25.14
(security, Rust data, React state, generated pages, concurrency/tests). About 60
distinct findings after de-duplication. This spec fixes all of them.

## Owner decisions

- **Sequencing: hotfix first.** The token host-check fix (A1) is Task 1 on its own
  branch and can ship alone as 1.25.15 when the owner says so. Everything else follows
  on one branch, shipped as one later release.
- **Shared Steps: preserve and merge.** Shared-step references are parsed, shown as a
  locked row, and kept on save; the steps XML is edited in place, not rebuilt.
- **Failed upload batch: reconcile, else hold.** After a failed `$batch`, look up what
  Azure DevOps actually created; if that lookup fails too, hold the rows as "outcome
  unknown" until a check succeeds.

## Global constraints

- No HTTP DELETE to Azure DevOps outside `ado/deletion.rs` (owner-authorised test-case
  deletion). Transport stays GET/POST/PATCH.
- Rust tests only under `src-tauri/tests/`. `src/bindings.ts` is regenerated with
  `cargo test --test bindings`, never hand-edited.
- `src/ui-consistency.test.ts` is never weakened. Colours only via tokens / palette
  variables. Icons from `src/lib/actionIcons.ts`.
- User-facing errors never contain a URL (`ado/transport.rs` sentences via
  `network_error`).
- Page JS under `src-tauri/web/` is ES5.
- One cache implementation per side (`src/lib/cache.ts`, `src-tauri/src/cache/`).
- Changelog is end-user prose.

## A. Security

| # | Finding | Fix |
|---|---|---|
| A1 | `work_board/detail.rs` `host_of` splits on `/?#` only; `https://evil\.dev.azure.com/...` passes the host check and the bearer token goes to `evil` (attachments and `get_avatar_b64`). | `host_of` parses with `reqwest::Url::parse` (WHATWG, the same parser reqwest connects with) and returns `(scheme, host)` from `host_str()`; any username/password refuses. `token_may_be_sent_to` keeps its allow-list (the client's own base host, `dev.azure.com`, `*.dev.azure.com`, `*.visualstudio.com`) applied to the parsed host. |
| A2 | `http://` accepted, so the token can go out in cleartext. | Microsoft domains require `https`. `http` is accepted only when scheme AND host equal the client's own base URL (an on-premises server the user configured). |
| A3 | `ado/endpoints.rs` `wiki_page_at` interpolates `wiki` unencoded; the bridge decodes it first, so `..%2F` reaches any GET endpoint. | `percent_encode_segment(wiki)`; the bridge also refuses a `wiki` containing `/`, `\` or `..`. |
| A4 | `auth.rs` token-endpoint errors use `to_string()`, which shows the login URL. | Route reqwest errors through `network_error`; test in `tests/ado_network.rs` style for `auth`. |
| A5 | Sign-in loopback (`auth.rs:168-233`): no accept/read timeout; a silent preconnect blocks; the "signed in" page shows even on state mismatch or denial. | Non-blocking accept with a 5-minute overall deadline; 10 s read timeout per connection; a connection without a `GET /?` request line is dropped and the loop keeps waiting; the success page only when state matches and a code is present, otherwise a failure page. Timeout returns a plain sentence ("Sign-in timed out. Try again."). |
| A6 | `note_server.rs:322` `body[..len]` on a lossy string can panic before the token check. | Slice bytes, then decode. |
| A7 | `import_parser/html.rs:479-486` file label in single-quoted attributes via `esc`. Also `html.rs:248` `href`. | `esc_attr` for every single-quoted attribute value. |
| A8 | `report.rs` outcome and org unescaped. | `esc` / `esc_attr`; add a viewport meta. |
| A9 | Bridge `/optimize` and `/transform` `in_place` write any path that parses as a draft. `save_draft_comment` / `save_draft_cases` skip `writable()`. | All four go through `writable()` (watched files) or the working repository's `.test-cases` folder. |
| A10 | `commands/ai_tools.rs` `cmd /C claude mcp add` passes env values that cmd.exe interprets. | Escape `^ & | < > ( ) %` and `"` for cmd; always quote values. |
| A11 | `deletion.rs:182` re-checks permission at the root area; the UI gates on the PBI's area. | Pass the PBI's area path into the re-check. |
| A12 | `state.rs:45-77` token refresh is not single-flight and can overwrite a newer sign-in. | A tokio mutex around refresh; after refresh, store only if the stored refresh token is still the one that was sent. |

## B. Transport

| # | Finding | Fix |
|---|---|---|
| B1 | No timeout on any reqwest client (`ado/mod.rs:172`, `auth.rs:110`); a new client per command. | One shared client (`OnceLock`) with `connect_timeout(10 s)` and `timeout(60 s)`; the `$batch` call sets a per-request `timeout(180 s)`. A timeout maps to the existing `NET_TIMEOUT` sentence. |
| B2 | Permanent delete ignores Retry-After / X-RateLimit-Delay. | Delete calls `throttle::pace` before and `throttle::note_server_delay` after, like `transport::send` (it keeps its own method, as the only DELETE). |
| B3 | Attachment upload (`endpoints.rs:1047`) and avatar GET (`detail.rs:249`) bypass `transport::send`. | Route both through `send`. |
| B4 | `BatchRequest.method` is a free string: a `"DELETE"` sub-request would pass the allow-list. | `enum BatchMethod { Patch, Post }`, serialised as `"PATCH"`/`"POST"`. |
| B5 | The DELETE test scans a hand-kept list of 13 files. | Walk every `src/**/*.rs`, excluding only `ado/deletion.rs`. |
| B6 | Accept loops (`ai_bridge.rs:2190`, `note_server.rs:122`) spin on persistent errors. | Back off 100 ms doubling to 5 s; reset on success. |
| B7 | Tag refresh (`commands/discovery.rs:71-85`) spawns one refresh per stale call; a late one can drop tags an upload just added. | Single-flight flag; the refresh merges into the current cache (union) instead of replacing it. |

## C. Upload integrity

| # | Finding | Fix |
|---|---|---|
| C1 | `commands/queue.rs:843-850`: a failed `$batch` marks every case failed although ADO may have created them; retrying creates duplicates. | On a failed batch that contained creates: run one WIQL for Test Cases linked (Tested By) to the PBI with `System.CreatedDate >= upload start` and title in the batch's create titles. Each create whose title matches exactly one unclaimed found item is reported `created` with that id. Remaining creates are `failed`. Updates in the batch stay `failed` (PATCH is idempotent to retry). If the lookup itself fails, remaining creates are reported with a new `action: "unknown"`. |
| C2 | Hold on unknown. | Rows whose last result is `unknown` are marked in the queue ("Outcome unknown - check before uploading again"). Upload refuses while any are present. A **Check** button on the result runs the same lookup (`reconcile_upload` command) and clears the mark: found → the row gets the id and is removed like a created case; not found → the row returns to normal. |
| C3 | No tests for create-vs-update and result mapping (`queue_item_request`, `sent_idx`). | Extract pure helpers (`queue_item_request`, `map_batch_results`, `match_reconciled`) as `pub` and test them in `tests/submit_mapping.rs`. |

## D. Steps XML

| # | Finding | Fix |
|---|---|---|
| D1 | `parse_steps_xml` ignores `<compref>`; any step edit rebuilds the XML without shared steps and renumbers ids. | `Step` gains `#[serde(default, skip_serializing_if = "Option::is_none")] shared: Option<i32>`. Parse emits a `<compref ref="N">` as `Step { action: "", expected: "", shared: Some(N) }` in document order (its child steps, if any, are not flattened). New `merge_steps_xml(original, steps) -> String`: tokenise the original into top-level nodes (`<step>` with id, `<compref>` with id and ref, kept verbatim); align old local steps to new local steps by LCS on (action, expected), then by position among the unmatched; emit in the new order: matched-unchanged → original node verbatim (retyped if needed); matched-changed → rebuilt `<step>` with the old id; new → rebuilt with ids from `last+1`; shared → the original `<compref>` for that ref (first unused one), or a fresh `<compref id="next" ref="N" />` if it was not in the original. `last` = highest id used. `steps_patch` uses `merge_steps_xml` whenever an original exists. |
| D2 | App side of shared steps. | Locked row in the step editors (`CaseStepsTable` and the queue/manual editors): shows "Shared steps #N" plus the title (fetched once per id via the existing work-item batch GET, cached with `persistentQuery`); cannot be edited; can be moved and removed. JSON import/export carries `"shared": N` on a step. `validate_cases` accepts a step with `shared` and no action. The writing guide mentions it in one line. |
| D3 | Step text is written with the XML layer only; ADO renders the inner HTML, so a typed `<cycleId>` vanishes in ADO's UI; `&lt;` typed double-decodes. | New/changed step text is `escape_xml(escape_html(text))` (`escape_html` = `& < > "`). The parser already undoes two layers; old single-escaped cases still parse the same. |
| D4 | `retype_steps_xml` breaks a self-closing `<step .../>` with no type. | Insert the attribute before a trailing `/`. |
| D5 | Verification. | Before release: one live case with a shared step edited through the app and inspected in ADO (owner or a live-org session). |

## E. Draft file integrity

| # | Finding | Fix |
|---|---|---|
| E1 | Writers re-parse raw text with serde; a UTF-8 BOM makes them fall back to a fresh document (losing `specs`, `comments`, unknown keys) or fail. | One `import_parser::read_json_text(path) -> String` (BOM stripped) used by every reader/writer (`export.rs`, `comments.rs`, `specs.rs`, `ai_bridge.rs`). Writers write without a BOM. |
| E2 | In-place rewrites replace the whole `test_cases` array with the parsed cases: skipped cases vanish, unknown per-case keys and alias spellings are lost. | `ParsedFile` records, per parsed case, the index of its source object in `test_cases`. `merge_cases_into_draft(old_text, cases)` patches: for each case with a source index, update only the modelled keys on that object (writing to the alias key the object already uses); objects the importer skipped are kept verbatim at their position; cases without a source index (inserted) are appended; cases removed by the operation are removed by source index. |
| E3 | `filewatch.rs:201` `write_watched` truncates in place; bridge in-place writes don't take `NOTE_WRITE`. | Temp file + rename in `write_watched`; the bridge's in-place writes take `NOTE_WRITE`. |
| E4 | `transform.rs:476-487` `title_contains: ""` passes the remove guard. | An empty or whitespace string does not count as a filter; the op is refused. |
| E5 | `transform.rs:528-531` `insert_cases` wraps ids, ignores string ids, blanks array tags, keeps empty steps. | Use the importer's id reader (positive i32, strings accepted, else error); tags via the importer's tag reader; refuse a case with no step with an action. |
| E6 | `comments.rs:37-49` matches only integer `id` and `title`. | Match via the importer's id and title readers (all aliases); clearing a comment removes every comment alias key. |
| E7 | `merge_case_files` drops slices' `comments`; relative `specs` resolve against the wrong folder. | Merged `comments` = each slice's non-empty comment joined with a blank line, prefixed by the slice file name; relative spec entries rewritten relative to the merged file's folder. |

## F. Queue and file sync (React)

| # | Finding | Fix |
|---|---|---|
| F1 | `ImportFile.tsx:325-412` a cancelled sync is never retried. | A `syncTick` state bumped in `finally` when the run was cancelled; it is an effect dependency. |
| F2 | `ImportFile.tsx:349-364` the queue is overwritten after an await. | Read the general comment before computing the sync; apply the queue change with a functional updater that recomputes `syncFromFile` on the latest queue. |
| F3 | `QueueSection.tsx:1026` single-row remove does not write back. | Call `writeBackOwned` as `bulkRemove` does. |
| F4 | `fileSync.ts:144-199, 295-324` repeat numbering includes hand-typed rows. | Number repeats over file-owned rows only (rows without an owner are keyed separately, never matched against a file). |
| F5 | `QueueSection.tsx:233-270` a cleared comment on an update case is refilled; edits never reach the View note. | Fill from the note only the first time a row with that update id appears in this queue (tracked in a ref set). A later edit to the comment writes the note (debounced). |
| F6 | `submitRun.ts` + `QueueSection.tsx:571-622` phase is global; a refused second submit clears the first's phase; listen calls outside `try`. | Phase carries the PBI; `submitFinished(pbi)` clears only its own; listens move inside `try`. |
| F7 | `QueueSection.tsx:814-822` watch stamp lost after a remount. | Always persist through `saveWatches` storage in addition to the live mount's callback. |
| F8 | `ImportFile.tsx:495-510` stale `setWatches` after a PBI switch. | The shared-import callback saves through a setter keyed to the PBI it started with. |
| F9 | `QueueSection.tsx:1639` floating Review bypasses `openReview()`. | Call `openReview()`. |
| F10 | `QueueSection.tsx:495-504, 247` browser comments go to every same-titled row and to whichever PBI is on screen. | Match the full occurrence key and the report's PBI; line 247 compares full keys. |

## G. Review page and Test map

| # | Finding | Fix |
|---|---|---|
| G1 | `html.rs:517` vs `:532` live update never runs. | Emit the globals script (`NOTE_PORT`, `NOTE_TOKEN`, `NOTE_ORG`, `REPORT_REV`, `REPORT_KIND`) before `cases-page.js`; `cases-notes.js` stays after. Test asserts the order. |
| G2 | Revision embedded one behind. | Bump before rendering and embed the bumped value. |
| G3 | After a swap, banner/Refresh/findings toggle are dead. | `banner()` looks the node up each time; Refresh via a delegated document click; `swap()` calls `wireFindingsToggle()` and re-applies the findings-off label. |
| G4 | Open-state restore keyed by the first `summary`, only reopens. | Key by the case `h2` text plus the details' class; restore both open and closed. |
| G5 | `test_map.rs:166` draft and queue pages share `test-map-{pid}.html`. | `test-map-{kind}-{pid}.html`. |
| G6 | `cases-specs.js:118` `\p{L}` / `u` flag is not ES5. | Replace with an ES5 approach: split on characters that are not letters/digits by testing `c.toLowerCase() !== c.toUpperCase()` or `/[0-9]/`. |
| G7 | Only the first `Spec:` per text node is linked; links inside `<a>` nest; extension-less multi-word names split wrongly. | Loop over every `Spec:` occurrence; skip text nodes inside `a`; `splitCitation` tries the longest known spec title prefix first. |
| G8 | Search bar doesn't wrap; long titles/cells overflow. | `flex-wrap: wrap` on `.searchbar`; `overflow-wrap: anywhere` on `.case h2` and `td`. |
| G9 | Many general-comment files squeeze the spec pane to zero. | `.side` gets `overflow:auto`; `.specs` gets `min-height: 240px`. |
| G10 | `spec_pane.rs` `first_heading` matches `#` in fenced code. | Skip lines inside ``` / ~~~ fences. |
| G11 | `cases-notes.js` autosaves unordered. | Per-box sequence number; a reply for an older sequence is ignored; a new save waits for the in-flight one. |
| G12 | `test-map.js:225` area labels overrun cases when zoomed out. | Clip the label to the gap before the child column (measure text, ellipsis when it does not fit); hidden entirely below the title-alpha threshold. |
| G13 | Hard-coded `#444` and `rgba` shadows in `cases-page.css`. | Palette variables (`--muted`, `--shadow`). |

## H. Leaks and test hygiene

| # | Finding | Fix |
|---|---|---|
| H1 | `audio.rs:136` exiting capture thread clears the new thread's slot. | Clear only if the slot's `stop` is `Arc::ptr_eq` its own. Test in `tests/audio.rs` via a pure slot helper. |
| H2 | `audioSpectrum.ts` listener registered after release. | After the awaited start, if refs is 0, unlisten immediately. |
| H3 | `webgl.ts` probe context never released. | `WEBGL_lose_context.loseContext()` after probing. |
| H4 | Wiki bodies accumulate in `cache.json`; each write rewrites the file under the global lock. | Evict `wiki-page:` entries older than 7 days and keep at most 50 (oldest first) on every put; persist outside the lock (snapshot under lock, write after). |
| H5 | `claim_for(None)` does nothing. | An unknown account wipes the cache like a different account. |
| H6 | `tests/bindings.rs` two tests write the same file in parallel; the token check only matches `access_token`. | The token check renders the bindings to a string (no file) and checks `access_token`, `refresh_token`, `id_token`, `bearer`. |
| H7 | `tests/ado.rs` 429 test leaves a 17 s global hold. | Reset the throttle state at the end of that test (the reset helper `throttle_backoff.rs` uses). |

## Testing

Every behaviour fix starts with a failing test: Rust in `src-tauri/tests/`, frontend in
vitest next to the existing tests for that module. Pure page logic is tested through
`window.tcmSpecs` / `window.testMap` style helpers loaded with `new Function`. The
review-page order (G1) is asserted on the generated HTML. The full gates (cargo test,
vitest, build) run before the branch is offered for merge.

## Out of scope

Code signing, and owner-documented decisions (shipped DB credentials, unsigned updates).

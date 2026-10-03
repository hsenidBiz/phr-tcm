# Auto Run iframe locators Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Auto Run scripts can find, click, fill and assert on elements inside a same-origin `<iframe>` through a locator chain that names the iframe as one step, and the page snapshot shows frame contents with pasteable chains.

**Architecture:** `browser/locator.rs` `resolve()` swaps an `<iframe>` match for its `contentDocument` whenever another chain step follows, and reports a frame it could not enter. `browser/input.rs` `PROBE_JS` turns frame-relative measurements into top-window coordinates and checks cover at every frame level. `browser/snapshot.rs` reads each iframe's own accessibility tree and prints it under the iframe's line.

**Tech Stack:** Rust (tokio), Chrome DevTools Protocol over `browser/cdp.rs`, in-page JavaScript strings, Edge headless for live tests.

**Spec:** `docs/superpowers/specs/2026-10-03-autorun-iframe-locators-design.md` (read it first).

## Global Constraints

- Same-origin iframes only; a cross-origin (or sandboxed, or not yet loaded) frame fails with exactly: `the frame <describe> holds a page from another site (or has not loaded), which Auto Run cannot reach` - `<describe>` is the iframe step's `LocatorStep::describe()` text.
- No change to the script format: `LocatorStep`, `Target`, the save gate's validation, `src/bindings.ts` (do not regenerate), the frontend.
- Legacy string selectors stay top-document only.
- The chain's LAST step is never swapped for a frame document.
- The recorder (`autorun/recorder.rs`, `autorun/signin_recorder.rs`) is not touched.
- Every Rust test is an integration test in the one `tests/suite` binary (CLAUDE.md); live browser tests are `#[ignore = "starts a real headless Edge"]` and run with `cargo test --test suite browser_live:: -- --ignored --test-threads=1`. Run one build/test command at a time.
- Commits: bash heredoc `git commit -F - <<'EOF' ... EOF`, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Confirm with `git log -1`.

## Review Focus

1. Two employee-search iframes on one page (one per dialog), only one visible: the iframe step must match the visible one only, so the chain does not report "matched 2". Test in Task 2.
2. `expect_hidden` / `expect_count 0` through an unreachable frame must FAIL with the cross-origin sentence, not pass because nothing was found. Test in Task 2.
3. An iframe inside a scrolled page with a border: the click must land on the inner element (border and scroll offsets), not beside it. Test in Task 3.
4. A parent-page overlay over the iframe: the action must wait and then report "covered by" the overlay, never click through it. Test in Task 3.
5. An iframe with no `id` and no `title`: the snapshot must still print a chain that resolves to the elements under it. Test in Task 4.

---

### Task 1: Frame fixture and the role-lookup spike

**Files:**
- Create: `src-tauri/tests/fixtures/autorun-iframe.html`
- Modify: `src-tauri/tests/suite/browser_live.rs` (helper + one test)

**Interfaces:**
- Produces: fixture ids below; `fn iframe_fixture_url() -> String` and `async fn open_iframes() -> Live` in `browser_live.rs` (same shape as `fixture_url()` / `open()`, navigating to the new fixture); the spike's two answers, recorded as a comment above the test: (a) does `Accessibility.queryAXTree` given the frame DOCUMENT's `objectId` return nodes inside the frame, (b) does `queryAXTree` with role `Iframe` (try also `iframe`) on the top document return the iframe element.

Fixture contents (mirror `phr.employee-search.js`: an iframe with no `src`, filled with `document.write`):
- A 1200px-tall spacer div before everything below, so the iframe needs scrolling.
- `#es-wrap` (margin-left 180px) holding `<iframe id="es-frame" title="Employee Search" style="border:7px solid #888;width:500px;height:260px">`, filled on load via `contentDocument.open()/write()/close()` with: `<button id="pick">Select</button>`, `<input id="q" aria-label="Search employees">`, `<div id="out"></div>`, and a script so `#pick` click sets `#out` text to `picked` and `#q` input sets `#out` to `typed:<value>`.
- A second, HIDDEN twin: `<div style="display:none"><iframe title="Employee Search"></iframe></div>` filled the same way.
- `<iframe id="locked" title="Locked frame" sandbox srcdoc="<button>Locked</button>">` (no `allow-same-origin`, so its `contentDocument` is null from the parent).
- `<iframe data-k="bare" style="width:300px;height:80px">` - deliberately NO `id` and NO `title` (Review Focus 5); the fixture script finds it by `data-k` and fills it with `<button>Bare inner</button>`.
- `<div id="cover" style="display:none;position:fixed;inset:0;background:rgba(0,0,0,.2)">` and a top-level `<button id="show-cover">Show cover</button>` that displays it.

- [ ] **Step 1: Write the fixture and `open_iframes()`**
- [ ] **Step 2: Write the spike test** `frame_spike_role_lookup_through_a_frame_document` - gets the iframe's `contentDocument` handle via `page::call_elements` on `#es-frame`, calls `Accessibility.queryAXTree` with `{objectId, role: "button"}` and prints/asserts the names it returns; then `queryAXTree` on the top document with role `Iframe`. Assert nothing yet beyond "the calls succeed"; print both answers.
- [ ] **Step 3: Run it**
  Run: `cd src-tauri && cargo test --test suite browser_live::frame_spike -- --ignored --test-threads=1 --nocapture`
  Expected: PASS, printing whether `Select` was found through the frame document and whether role `Iframe` matched.
- [ ] **Step 4: Record the answers** as a comment above the test, then turn the prints into assertions of what was observed (so a Chrome change that breaks the chosen path fails loudly).
- [ ] **Step 5: Commit** (`test(v2): iframe fixture and the role-lookup spike for Auto Run frames`)

### Task 2: Resolver enters frames and explains a frame it cannot enter

**Files:**
- Modify: `src-tauri/src/browser/locator.rs` (`resolve`, `by_role`, new items below)
- Modify: `src-tauri/src/browser/input.rs:229-275` (`look`: use the explanation)
- Modify: `src-tauri/src/browser/expect.rs:74-160` (`look`: use the explanation)
- Test: `src-tauri/tests/suite/browser_locator.rs`, `src-tauri/tests/suite/browser_live.rs`

**Interfaces:**
- Consumes: Task 1 fixture, `open_iframes()`, and the spike answer (a).
- Produces (in `browser/locator.rs`):
  - `pub struct Resolved { pub handles: Vec<Handle>, pub unreachable_frame: Option<String> }`
  - `pub async fn resolve_explained<D: Driver>(d: &mut D, target: &Target) -> Result<Resolved, CdpError>`
  - `pub async fn resolve<D: Driver>(d: &mut D, target: &Target) -> Result<Vec<Handle>, CdpError>` - unchanged signature, returns `resolve_explained(..).handles`
  - `pub fn frame_unreachable(frame: &str) -> String` - the Global Constraints sentence
  - `pub const FRAME_JS: &str` - `this` is an element; returns by value `"frame"` (an IFRAME/FRAME with a reachable `contentDocument`), `"unreachable"` (IFRAME/FRAME, `contentDocument` null), or `"element"`; check `tagName`, never `instanceof` (the element may come from another realm)
  - `pub const FRAME_DOC_JS: &str` - `this` is a reachable frame element; returns `[this.contentDocument]` for `page::call_elements`

- [ ] **Step 1: Write the failing pure test** in `browser_locator.rs`:

```rust
#[test]
fn an_unreachable_frame_is_explained_in_a_sentence() {
    assert_eq!(
        frame_unreachable("iframe#locked"),
        "the frame iframe#locked holds a page from another site (or has not loaded), which Auto Run cannot reach"
    );
}
```

- [ ] **Step 2: Write the failing live tests** in `browser_live.rs` (all `#[ignore = "starts a real headless Edge"]`, all use `open_iframes()`, chain `[{"css":"iframe[title='Employee Search']"}, ...]` = `ES` below):
  - `frame_a_chain_reads_inside_the_frame`: `expect_text` on `ES + {"css":"#pick"}` equals `Select`; `expect_count` on `ES + {"role":"button","name":"Select","exact":true}` equals 1 (proves role steps inside the frame AND that the hidden twin iframe is not counted - Review Focus 1).
  - `frame_the_last_step_keeps_the_iframe_itself`: `expect_visible` on `{"css":"#es-frame"}` passes; `expect_count` on `[{"css":"#es-wrap"},{"css":"iframe"}]` equals 1.
  - `frame_an_unreachable_frame_is_named_not_reported_missing`: `expect_visible`, `expect_hidden` and `expect_count` (equals 0) on `[{"css":"#locked"},{"role":"button","name":"Locked"}]` each fail with detail containing `holds a page from another site` (Review Focus 2), and `click` on it fails the same way within the action timeout.
- [ ] **Step 3: Run them to see them fail**
  Run: `cd src-tauri && cargo test --test suite browser_locator::an_unreachable_frame` then `cargo test --test suite browser_live::frame_ -- --ignored --test-threads=1`
  Expected: FAIL (`frame_unreachable` not defined; live tests "not found").
- [ ] **Step 4: Implement** in `locator.rs`: in `resolve_explained`, after a step's matches are narrowed by `nth` and only if a later step exists, map each match through `FRAME_JS`: `"frame"` -> its document via `FRAME_DOC_JS`; `"unreachable"` -> drop it and set `unreachable_frame = Some(frame_unreachable(&step.describe()))` (first one wins); `"element"` -> keep. Role steps: if spike answer (a) was YES, `by_role` needs no change. If NO, keep the owning iframe handle with each document root (e.g. `struct Root { node: Handle, frame_owner: Option<Handle> }`) and, for a root with an owner, have `by_role` read `DOM.describeNode({objectId: owner}).node.frameId`, call `Accessibility.getFullAXTree({frameId})`, and apply the same role / name / `ignored` / visibility filters to its nodes (resolving each `backendDOMNodeId` with `page::resolve_backend`).
- [ ] **Step 5: Use the explanation** in `input.rs` `look` (empty handles + `unreachable_frame` -> `Look::NotYet { why: <that sentence>, rect: None }` instead of `NOT_FOUND`) and in `expect.rs` `look` (empty handles + `unreachable_frame` -> every `Check` returns `Err(<that sentence>)` before its own logic, so `Hidden` and `Count(0)` cannot pass vacuously).
- [ ] **Step 6: Run the tests to see them pass** - same commands as Step 3, plus the whole suite once: `cd src-tauri && cargo test --test suite browser_` - Expected: PASS, no existing test changed.
- [ ] **Step 7: Commit** (`feat(v2): Auto Run locator chains reach inside same-origin iframes`)

### Task 3: Clicks and typing land inside a frame

**Files:**
- Modify: `src-tauri/src/browser/input.rs:55-95` (`PROBE_JS` only)
- Test: `src-tauri/tests/suite/browser_live.rs`

**Interfaces:**
- Consumes: Task 2 resolver; Task 1 fixture.
- Produces: `PROBE_JS` returns the same fields as today (`visible, enabled, editable, onscreen, hit, x, y, rect, covered_by`), with `x`, `y` and `rect` in TOP-WINDOW coordinates and `onscreen` / `hit` / `covered_by` judged at every frame level.

- [ ] **Step 1: Write the failing live tests:**
  - `frame_a_click_lands_on_the_element_inside_a_scrolled_bordered_frame`: `click` on `ES + {"css":"#pick"}` passes, then `expect_text` on `ES + {"css":"#out"}` equals `picked` (Review Focus 3).
  - `frame_typing_reaches_an_input_inside_the_frame`: `fill` `ES + {"role":"textbox","name":"Search employees"}` with `Ethan`, then `expect_text` on `ES + {"css":"#out"}` equals `typed:Ethan`.
  - `frame_a_parent_overlay_over_the_frame_is_reported_as_covering`: click `#show-cover`, then `click` on `ES + {"css":"#pick"}` fails with detail containing `covered by` and `div#cover`; `#out` is still empty (Review Focus 4).
- [ ] **Step 2: Run them to see them fail**
  Run: `cd src-tauri && cargo test --test suite browser_live::frame_ -- --ignored --test-threads=1`
  Expected: the three new tests FAIL (click misses / no cover detected).
- [ ] **Step 3: Implement in `PROBE_JS`** after the existing in-frame measurement:

```js
// Walk out to the top window: translate the point and the rect, clip to each
// frame's visible box, and require each enclosing level to hit its frame element.
let w = window, ox = 0, oy = 0, clip = { l, r, t, b: bt }, outerTop = null;
while (w.frameElement) {
  const fe = w.frameElement, fr = fe.getBoundingClientRect();
  const dx = fr.left + fe.clientLeft, dy = fr.top + fe.clientTop;
  ox += dx; oy += dy;
  clip = { l: Math.max(clip.l + dx, fr.left), r: Math.min(clip.r + dx, fr.right),
           t: Math.max(clip.t + dy, fr.top), b: Math.min(clip.b + dy, fr.bottom) };
  const pw = w.parent;
  clip = { l: Math.max(clip.l, 0), r: Math.min(clip.r, pw.innerWidth), t: Math.max(clip.t, 0), b: Math.min(clip.b, pw.innerHeight) };
  const px = x + ox, py = y + oy;
  const there = pw.document.elementFromPoint(px, py);
  if (!there || (there !== fe && !fe.contains(there))) { outerTop = outerTop || there; }
  w = pw;
}
```

  Then report `x + ox`, `y + oy`, `rect` shifted by `(ox, oy)`, `onscreen` = in-frame `onscreen` AND the final `clip` is non-empty, `hit` = in-frame `hit` AND `outerTop === null`, and `covered_by` = `say(outerTop)` when the outer check failed (else today's value). With no `frameElement` the loop does nothing, so top-level elements behave exactly as before.
- [ ] **Step 4: Run the tests** - the Step 2 command, then the whole live module once: `cd src-tauri && cargo test --test suite browser_live:: -- --ignored --test-threads=1` - Expected: PASS, including every pre-existing live test.
- [ ] **Step 5: Commit** (`feat(v2): Auto Run clicks and types inside same-origin iframes`)

### Task 4: The snapshot shows frame contents; guide and tool text

**Files:**
- Modify: `src-tauri/src/browser/snapshot.rs` (`AxNode`, `parse_nodes`, new `FrameTree`, `render_frames`, `render`, `snapshot`)
- Modify: `src-tauri/src/autorun/guide.rs:403-405` (the iframe paragraph)
- Modify: `src-tauri/src/mcp.rs:352` and `src-tauri/src/ai_tools.rs:159` (`probe_autorun_locator` description: one added sentence)
- Test: `src-tauri/tests/suite/browser_snapshot.rs`, `src-tauri/tests/suite/browser_live.rs`

**Interfaces:**
- Consumes: Task 2 resolver (the printed chains must resolve); spike answer (b).
- Produces (in `browser/snapshot.rs`):
  - `AxNode` gains `pub backend: Option<i64>` (from `backendDOMNodeId`); update the `node()` helper in `browser_snapshot.rs` with `backend: None`.
  - `pub struct FrameTree { pub iframe_id: String, pub step: serde_json::Value, pub nodes: Vec<AxNode>, pub unreadable: Option<String> }` - `iframe_id` is the `Iframe` node's AX `id` in the parent tree.
  - `pub fn render_frames(nodes: &[AxNode], frames: &[FrameTree], limit: usize) -> String`; `render(nodes, limit)` becomes `render_frames(nodes, &[], limit)`.

Line format inside a frame: the frame's nodes are walked by the existing rules, indented one level deeper than the iframe's line, and each line's suffix is ` -> ` + the JSON array `[step, <the line's own locator object>]` (nested frames prepend each frame's step). An unreadable frame prints one line under the iframe: `(frame contents could not be read: <why>)`. The `limit` counts every printed line.

Iframe step (built in `snapshot()`, from `DOM.describeNode({backendNodeId})`): if spike (b) said role `Iframe` matches AND the node has a name -> `{"role":"Iframe","name":<name>,"exact":true}`; else if the element has an `id` -> `{"css":"iframe#<id>"}`; else if it has a `title` -> `{"css":"iframe[title='<title, single quotes escaped>']"}`; else `{"css":"iframe","nth":<k>}` where `k` is the zero-based order of this `Iframe` node among the parent tree's `Iframe` nodes (Review Focus 5).

- [ ] **Step 1: Write the failing pure tests** in `browser_snapshot.rs`:
  - `a_frame_tree_prints_under_its_iframe_with_chained_locators`: parent nodes root -> `Iframe` "Employee Search" (id `f1`); `FrameTree { iframe_id: "f1", step: json!({"css":"iframe[title='Employee Search']"}), nodes: [RootWebArea -> button "Select"] }`; assert the output contains a line ending `-> [{"css":"iframe[title='Employee Search']"},{"name":"Select","role":"button"}]` (key order as `json!` serialises) indented deeper than the `Iframe` line.
  - `an_unreadable_frame_prints_one_line_saying_so`: same parent, `unreadable: Some("no frame id")`, `nodes: []`; output contains `(frame contents could not be read: no frame id)`.
  - `the_line_limit_counts_frame_lines`: a frame with 5 buttons, limit 4; output has exactly 4 node lines plus the existing truncation marker.
- [ ] **Step 2: Run them to see them fail**
  Run: `cd src-tauri && cargo test --test suite browser_snapshot::`
  Expected: FAIL (`FrameTree` / `render_frames` not defined).
- [ ] **Step 3: Implement** `parse_nodes` (`backend`), `FrameTree`, `render_frames`, and in `snapshot()`: after parsing the top tree, for each `Iframe` node with a `backend`, `DOM.describeNode` it (frame id = `node.frameId`, attributes for `id` / `title`), then `Accessibility.getFullAXTree({frameId})` -> `parse_nodes`; a failure at any point becomes `unreadable: Some(<message>)`. Recurse for `Iframe` nodes inside a frame tree.
- [ ] **Step 4: Write and run the live check** `frame_the_snapshot_prints_paste_ready_chains` in `browser_live.rs`: take `snapshot(&mut live.cdp, DEFAULT_LIMIT)`, find the line naming button `Select`, parse the JSON after ` -> `, and `click` with it as the selector - PASS, then `expect_text` on `ES + {"css":"#out"}` equals `picked`. Also find the `Bare inner` button's line and assert its chain resolves (`expect_count` equals 1). The `#locked` iframe shows an unreadable line.
  Run: `cd src-tauri && cargo test --test suite browser_snapshot:: && cargo test --test suite browser_live::frame_ -- --ignored --test-threads=1`
  Expected: PASS.
- [ ] **Step 5: Replace the guide paragraph** at `guide.rs:403-405` with: a chain reaches inside a same-origin iframe by naming the iframe as one step and what is inside as the next (example `[{"css":"iframe[title='Employee Search']"},{"role":"button","name":"Search"}]`); `get_autorun_page` prints those chains under the iframe; a cross-origin iframe cannot be reached - a step that depends on one is left for the person. Add to the `probe_autorun_locator` description in both `mcp.rs` and `ai_tools.rs`: "A chain can pass through a same-origin iframe: name the iframe as one step and the element inside as the next."
- [ ] **Step 6: Run the gates once each**
  Run: `cd src-tauri && cargo test --tests` then `cd .. && npx tsc --noEmit`
  Expected: all PASS (`bindings.rs` passes unchanged - no `bindings.ts` diff).
- [ ] **Step 7: Commit** (`feat(v2): Auto Run snapshot shows iframe contents; guide explains frame chains`)

### Task 5: Prove it on PeoplesHR

**Files:** none in the repo (Auto Run scripts are app data).

- [ ] **Step 1:** Build and run the dev app (`npm run tauri dev`), open a supervised Auto Run browser as `conrad` on the PeoplesHR sandbox, open draft "FY2027 Sales Operations Performance Review" on Participants, click Search Employees.
- [ ] **Step 2:** `get_autorun_page`: the Employee Search iframe's contents print with chains. `probe_autorun_locator` on a printed chain reports 1 match. `try_autorun_action` a search and a row pick inside the iframe, then confirm the dialog: the employee appears in the grid.
- [ ] **Step 3:** Report the result and the working chains, so the 15 skipped Participants cases (137522-137525, 137544, 137561, 137564, 137571, 137574, 137583-137585, 137587, 137588, 140919) can be scripted.

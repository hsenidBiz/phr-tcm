# Auto Run: locators that reach inside an iframe

Design, agreed with the owner on 2026-10-03.

## 1. Why

PeoplesHR's shared employee picker, the `<phr-employee-search>` Common
Component, renders its whole search form and results table inside an
`<iframe>`. On the Performance Cycle wizard it is behind Search Employees and
the Assign Manager / Assign Reviewer dialogs on the Participants step, and it
is reused in other modules. Auto Run's actions run in the top document only
(`autorun/guide.rs`: "Content inside an `<iframe>` cannot be reached at
all"), so 15 of the 79 live Participants cases on PBI 135592 were skipped
(137522-137525, 137544, 137561, 137564, 137571, 137574, 137583-137585,
137587, 137588, 140919). Picking an employee is necessary for those cases.

The iframe is same-origin: the component fetches the Common Components HTML
and `document.write`s it into an iframe that has no `src`
(`phr.employee-search.js`, `createIframe` / `injectContentIntoIframe`), so
the page's own JavaScript can reach `iframe.contentDocument`.

Success means:

- A script can find, click, fill and assert on elements inside a same-origin
  iframe, with a locator chain that passes through the iframe.
- The page snapshot an assistant authors scripts from shows the iframe's
  contents, each line carrying a locator chain it can paste.
- A chain that tries to enter a cross-origin iframe fails with a sentence
  that says why, not "not found".
- No saved script changes behaviour, and the script format does not change.

## 2. Owner decisions

1. **Approach A**: a chain step that matches an `<iframe>` hands the NEXT
   step that iframe's document to search. No new locator field, no
   "switch to frame" action.
2. **No recording.** The recorder is not extended into frames; only replay,
   the snapshot, the probe and the guide are.
3. **Same-origin only.** Cross-origin iframes stay unreachable, with a clear
   failure.

## 3. Design

### 3.1 Finding elements (`browser/locator.rs`)

`resolve()` walks a chain step by step, each step searching inside the
previous step's matches. Today a match that is an `<iframe>` element yields
nothing for the next step: neither `querySelectorAll` (CSS and text steps)
nor `Accessibility.queryAXTree` (role steps) descends into a frame.

Change: after a step's matches are collected, and only when another step
follows, each match that is an `<iframe>` element is replaced by its
`contentDocument`. The CSS, text and role searches are unchanged; they
receive a document root.

- **The last step is never swapped.** When the iframe is the chain's target,
  it stays the iframe element, so `expect_visible` on the iframe still means
  "the frame is there".
- **Nested frames** work the same way, one step per frame.
- **No reachable document** (a cross-origin frame, or one whose document is
  not there yet): that match yields nothing for the next step, and the
  resolver records why. When the action's wait ends with nothing found and
  such a frame was met, the failure says: "the frame <describe> holds a page
  from another site (or has not loaded), which Auto Run cannot reach".
  A not-yet-loaded frame is covered by the actions' existing wait.
- **Role steps:** `by_role` passes the iframe document's `objectId` to
  `Accessibility.queryAXTree`. Whether Chrome descends into a frame document
  passed that way is proven first (section 5). If it does not, `by_role`
  instead reads that frame's tree with
  `Accessibility.getFullAXTree({ frameId })` - the frame id from
  `DOM.describeNode` on the iframe element - and applies the same role,
  name, `ignored` and visibility filters.
- **The iframe's own step** can be any existing kind, for example
  `{ "css": "iframe[title='Employee Search']" }`, or
  `{ "role": "Iframe", "name": "Employee Search" }` if Chrome's role works
  with `queryAXTree` (proven in section 5).
- **Unchanged:** the script format (`LocatorStep`, `Target`), the save gate's
  locator validation, `describe` (e.g. `button "Search" in
  iframe[title='Employee Search']`), and legacy string selectors, which stay
  top-document only.

No saved script changes behaviour: searching inside an iframe element's own
children always found nothing, so no existing chain relies on it.

### 3.2 Acting on elements (`browser/input.rs`, `PROBE_JS`)

`PROBE_JS` runs in the element's own frame (`Runtime.callFunctionOn` uses
the object's realm), so its measurements are frame-relative. After its
`scrollIntoView` (which also scrolls the enclosing documents):

- **Coordinates:** walk `window.frameElement` up to the top window, adding
  each frame element's `getBoundingClientRect()` left/top plus its
  `clientLeft` / `clientTop`. The returned `x` / `y` are top-window
  coordinates, which is what `Input.dispatchMouseEvent` needs.
- **On screen:** the element's box is clipped against every enclosing
  frame's visible box and the top viewport, not only its own frame's.
- **Covered:** `elementFromPoint` inside the element's frame as today; then,
  at each enclosing level, the translated point must land on that frame
  element (or inside it). Otherwise `covered_by` names the parent-document
  element on top, so an overlay over the iframe is reported like any other
  cover.
- **`rect`** (the holding-still comparison) becomes top-window coordinates
  too, so a frame that moves still reads as moving.

`FOCUS_JS`, `Input.insertText` and key presses need no change: focus works
inside a same-origin frame and keys go to the focused element. All `expect_`
checks use `resolve`, so they follow 3.1.

### 3.3 Seeing it (`browser/snapshot.rs`)

`snapshot()` reads `Accessibility.getFullAXTree({})`, which covers the main
frame only; an iframe shows as an `Iframe` node with no children.

Change: for each `Iframe` node met while rendering, resolve its DOM node,
read its frame's tree (`DOM.describeNode` for the frame id, then
`getFullAXTree({ frameId })`), and render that tree indented beneath the
iframe's line. Every line inside carries a full locator chain that starts
with a step for the iframe - its role and name when it has a name AND the
section 5 spike shows `role: "Iframe"` matches with `queryAXTree`; otherwise
a CSS step built from its `id` (`iframe#<id>`) or else its `title`
(`iframe[title='<title>']`) - so it pastes into a script as-is.
A frame whose tree cannot be read prints one line saying so. The existing
line limit counts the frame's lines.

`probe()` uses `resolve`, so `probe_autorun_locator` reaches frames
unchanged. Its tool description gains one sentence that chains can pass
through a same-origin iframe.

### 3.4 Documenting it (`autorun/guide.rs`)

The paragraph at `guide.rs:403` is replaced: a chain reaches inside a
same-origin iframe by naming the iframe as one step and what is inside as
the next; the snapshot prints those chains; a cross-origin iframe cannot be
reached, and a step that depends on one is left for the person.

## 4. Out of scope

- The recorder (`autorun/recorder.rs`, `signin_recorder.rs`).
- Cross-origin and out-of-process iframes.
- Shadow DOM, and any frontend, script-editor or bindings change.

## 5. Testing

All Rust tests are integration tests in the one `tests/suite` binary, one
module per file.

- **Spike, first:** a `browser_live.rs` test on a real browser with a page
  holding a same-origin `srcdoc` iframe, proving whether
  `Accessibility.queryAXTree` finds a role inside the frame when given the
  frame document's `objectId`, and whether `role: "Iframe"` matches the
  iframe. Its answer picks the 3.1 role path.
- **`browser_locator.rs`** (scripted driver): an iframe match is swapped for
  its document only when another step follows; the last step keeps the
  iframe; a frame with no reachable document yields nothing and produces the
  cross-origin sentence.
- **`browser_input.rs`:** the probe's frame offset arithmetic and the
  parent-level cover check.
- **`browser_live.rs`** (real browser): a page with a same-origin `srcdoc`
  iframe, offset and scrolled, holding a button and an input. A click lands
  on the button, a fill reaches the input, a parent-page overlay over the
  iframe is reported as covering, and the snapshot prints the frame's lines
  with full chains.
- **`browser_snapshot.rs`:** frame trees render indented under the iframe
  line with prefixed locators; an unreadable frame prints its one line.

After it ships, the 15 skipped Participants cases are scripted against the
live employee search.

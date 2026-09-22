# Notification Deep Links, Review Page Scroll and Filters, Suite as Current PBI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Four requests from the owner on 2026-09-22: (1) clicking a notification opens that work item or pull request inside the app instead of the browser; (2) on the View in Browser page, following a "Spec:" citation link scrolls only the spec pane, never the test cases, so the page title stays put; (3) the review page's search can be narrowed to one field (title, ID, prerequisites, steps, tags, module); (4) a requirement suite in Search Suites can be made the current PBI without leaving the screen.

**Architecture:** Each item is one task with its own tests and commit. (1) gives `AppNotification` a structured `target`, the bell an `onOpen` callback, and `App.tsx` a one-shot `workFocus` that the Board and the Pull Requests panel consume through the same "focus prop + handled callback" pattern `manageFocus` already uses. (2) and (3) are changes to the served review page (`src-tauri/web/*.js` and the HTML built in `src-tauri/src/import_parser/html.rs`), tested by loading the real scripts into jsdom as the existing page tests do. (4) is a new "More" action on `Suites.tsx` that resolves the PBI's real title through the existing `search_pbis` command.

**Tech Stack:** React 19 + TypeScript (vitest, Testing Library, `mockIPC`), Rust (Tauri 2; string-assertion tests over the generated HTML), the review page's plain browser JavaScript.

**Reading of the owner's item 2 (state it in the report if the code disagrees):** the only link on the review page that crosses from a test case into the spec is the "Spec:" citation in a case's reviewer notes (`cases-specs.js` `wireCitations` → `jumpTo`). `jumpTo` calls `h.scrollIntoView({ block: 'start' })`, and `scrollIntoView` scrolls every scrollable ancestor - the `.spec-doc` article AND the window - so the page scrolls too and the `<h1>` at the top of `.page` goes off screen. The fix scrolls the article alone.

## Global Constraints

- Every Rust test is an integration test under `src-tauri/tests/`. One build or test command at a time on this shared machine; Rust from `src-tauri/` with `CARGO_TARGET_DIR=target/gate`; frontend `npx vitest run --exclude "**/.claude/**"`. No new crates or npm packages. `src/bindings.ts` is generated; no task here changes a command signature, so it must not change.
- GET/POST/PATCH only against Azure DevOps; nothing here adds an Azure DevOps call beyond the existing `search_pbis`.
- The review page's scripts are re-runnable after a live swap (`cases-page.js` `swap()` replaces the whole `.shell`/`.page` subtree and re-wires); any new control's state must be captured before the swap and restored after it, exactly as the search text is.
- No em dashes in user-facing text. Colours via tokens; icons from `src/lib/actionIcons.ts` (named for the action); `src/ui-consistency.test.ts` must not be weakened. The changelog is the owner's at release time; each commit subject states its user-visible change.
- Commits via Bash heredoc with the model's own `Co-Authored-By` trailer; confirm with `git log -1`.
- `src/App.test.tsx` mounts the whole app and has a documented load-induced flake; one failure there that passes on a re-run is that, two different ones are not.

**Out of scope:** notifications for anything but the four existing kinds; a PR detail screen (the Pull Requests panel's expanded row is the destination); highlighting the board card (the item's drawer is the destination); saved filters on the review page; resolving a suite's PBI when the suite is not a requirement suite.

## Tasks

1. A notification opens its work item or pull request in the app
2. The review page: a spec citation scrolls the spec pane only
3. The review page: search one field
4. Search Suites: use a requirement suite as the current PBI

---

### Task 1: A notification opens its work item or pull request in the app

**Files:**
- Modify: `src/lib/notifications.ts:20-31` (type), `:146-159`, `:162-196`, `:198-210` (sources)
- Modify: `src/components/NotificationBell.tsx:50` (props), `:143-152` (the title button)
- Modify: `src/components/ContextBar.tsx:19-40` (props), `:156` (`<NotificationBell>`)
- Modify: `src/App.tsx:180` (state beside `manageFocus`), `:950-962` (`<ContextBar>` props), `:1080-1100` (`<WorkBoard>` / `<PrPanel>` props)
- Modify: `src/screens/WorkBoard.tsx:131` (props), `:241` (`openItem`), `:723-740` (the drawer already opens any id)
- Modify: `src/screens/PrPanel.tsx:380-390` (`PrRow` props), `:456-462` (row root), `:642-700` (`PrGroup`), `:712` (`PrPanel` props), `:855-890`, `RepoPrSection`
- Test: `src/lib/notifications.test.ts`, `src/components/NotificationBell.test.tsx`, `src/screens/WorkBoard.test.tsx`, `src/screens/PrPanel.test.tsx`, `src/App.test.tsx`

**Interfaces:**
- Produces, in `src/lib/notifications.ts`:
  ```ts
  export type NotificationTarget =
    | { kind: "work-item"; id: number }
    | { kind: "pr"; repo: string; id: number };
  export type AppNotification = { /* existing fields */ target?: NotificationTarget };
  ```
  `noteAssigned` sets `target: { kind: "work-item", id: i.id }`; `notePrOverview` and `notePrComments` set `target: { kind: "pr", repo: pr.repo, id: pr.id }`. `href` stays as it is (the browser fallback and the secondary "Open in Azure DevOps" control). Notifications already stored in `localStorage` from before this change have no `target` and keep opening the browser.
- `NotificationBell({ org, onOpen }: { org: string; onOpen?: (target: NotificationTarget) => void })`. The title button: when `n.target && onOpen` it calls `onOpen(n.target)` and closes the panel (`setOpen(false)`), `title="Open in the app"`; otherwise it keeps today's `openUrl(n.href!)`. A notification with both `target` and `href` also gets a small secondary button after the title, `aria-label={`Open in Azure DevOps: ${n.title}`}`, `title="Open in Azure DevOps"`, `<IconOpenExternal size={12} aria-hidden />` (add `ExternalLink as IconOpenExternal` to `src/lib/actionIcons.ts` if no alias for `ExternalLink` exists yet; if one does, use it), calling `openUrl(n.href)`.
- `ContextBar` gains `onOpenNotification?: (target: NotificationTarget) => void` and passes it as `onOpen` to `<NotificationBell>`.
- `App.tsx`: `const [workFocus, setWorkFocus] = useState<NotificationTarget | null>(null);` next to `manageFocus`. Handler passed to `ContextBar`:
  ```ts
  onOpenNotification={(target) => {
    logUi(`nav: notification/${target.kind}`);
    setWorkFocus(target);
    setWorkSection(target.kind === "pr" ? "prs" : "board");
    setWorkMode(true);
  }}
  ```
  `<WorkBoard org project focusItem={workFocus?.kind === "work-item" ? workFocus.id : null} onFocusHandled={() => setWorkFocus(null)} />` and `<PrPanel org project focus={workFocus?.kind === "pr" ? workFocus : null} onFocusHandled={() => setWorkFocus(null)} />`.
- `WorkBoard({ org, project, focusItem = null, onFocusHandled })`: one effect: when `focusItem != null && board.data` → `setOpenItem(focusItem); onFocusHandled?.()`. The drawer at `:723` already opens any id (`WorkItemDrawer` fetches by id; `states` is `[]` for an item outside the loaded board, which only disables the state dropdown).
- `PrPanel({ org, project, focus = null, onFocusHandled })`: `focus` flows to `PrGroup` and `RepoPrSection` and then to each `PrRow` as `focused={focus != null && focus.repo === pr.repo && focus.id === pr.id}` with `onFocused={onFocusHandled}`. `PrRow`: an effect on `focused` becoming true does `setOpen(true)`, `rootRef.current?.scrollIntoView({ block: "center" })`, sets a `flash` state that adds `ring-2 ring-accent` to the row root for 2500 ms (`setTimeout`, cleared on unmount), then calls `onFocused?.()`. `PrPanel` itself: an effect when `focus != null && overview.isSuccess`: if no PR in `overview.data.awaiting`, `overview.data.mine` (when `showYours`) or the tracked repos' names (`trackedRepos.some((r) => r.name === focus.repo)`) can hold it, `toast.info(`Pull request !${focus.id} is not listed here. Track the ${focus.repo} repository to see it.`)` and `onFocusHandled?.()`. If a tracked repo could hold it but no row claims it within 4000 ms (paged out), the same toast without the second sentence: `Pull request !${focus.id} is not on this page.`; the timer is cleared when a row claims the focus or `focus` changes.

- [ ] **Step 1: Write the failing tests**

`src/lib/notifications.test.ts` - add:
```ts
test("sources carry a structured target beside the browser href", () => {
  noteAssigned("acme", "Web", [{ id: 501, title: "Wire login", work_item_type: "Task", state: "New" }]);
  notePrOverview("acme", "Web", {
    mine: [prFixture(12, { has_conflicts: true })],
    awaiting: [prFixture(13)],
  });
  notePrComments("acme", "Web", prFixture(14), 2);
  const byId = Object.fromEntries(list("acme").map((n) => [n.id, n.target]));
  expect(byId["assigned:501"]).toEqual({ kind: "work-item", id: 501 });
  expect(byId["pr-conflict:web:12"]).toEqual({ kind: "pr", repo: "web", id: 12 });
  expect(byId["pr-review:web:13"]).toEqual({ kind: "pr", repo: "web", id: 13 });
  expect(byId["pr-comments:web:14:2"]).toEqual({ kind: "pr", repo: "web", id: 14 });
});
```
(`prFixture` builds a `PullRequest` with `repo: "web"`, `status: "active"`; reuse the file's existing fixture if it has one; `list` is whatever accessor the file already uses to read the store.)

`src/components/NotificationBell.test.tsx` - add:
```ts
test("a notification with a target opens in the app and closes the panel; the browser stays one click away", () => {
  raise("acme", [
    { id: "assigned:501", kind: "assigned", title: "Task #501 assigned to you", body: "Wire the login flow",
      href: "https://x/501", target: { kind: "work-item", id: 501 } },
  ]);
  const opened: unknown[] = [];
  render(<NotificationBell org="acme" onOpen={(t) => opened.push(t)} />);
  fireEvent.click(screen.getByRole("button", { name: "Notifications, 1 unread" }));
  fireEvent.click(screen.getByRole("button", { name: "Task #501 assigned to you" }));
  expect(opened).toEqual([{ kind: "work-item", id: 501 }]);
  expect(openUrl).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog", { name: "Notifications" })).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Notifications" }));
  fireEvent.click(screen.getByRole("button", { name: "Open in Azure DevOps: Task #501 assigned to you" }));
  expect(openUrl).toHaveBeenCalledWith("https://x/501");
});

test("a stored notification without a target still opens the browser", () => {
  seed(); // pr-conflict:Web:10 has href only
  render(<NotificationBell org="acme" onOpen={() => { throw new Error("must not be called"); }} />);
  fireEvent.click(screen.getByRole("button", { name: "Notifications, 2 unread" }));
  fireEvent.click(screen.getByRole("button", { name: "PR #10 has merge conflicts" }));
  expect(openUrl).toHaveBeenCalledWith("https://x/10");
});
```
(`openUrl` is the mocked import: `import { openUrl } from "@tauri-apps/plugin-opener"` after the existing `vi.mock`.)

`src/screens/WorkBoard.test.tsx` - add, using the file's existing `mockIPC` board setup and `render` helper:
```ts
test("a focused item opens its drawer once the board has loaded, then reports handled", async () => {
  const handled = vi.fn();
  renderBoard({ focusItem: 12, onFocusHandled: handled }); // extend the helper to forward extra props
  expect(await screen.findByRole("dialog", { name: /Fix bug|#12/ })).toBeInTheDocument();
  expect(handled).toHaveBeenCalledTimes(1);
});
```
(Match the drawer's real accessible name: look at `WorkItemDrawer.tsx`'s `role="dialog"`/`aria-label` and the mocked `work_item_detail` command the file already provides, or add that mock.)

`src/screens/PrPanel.test.tsx` - add:
```ts
test("a focused pull request expands, scrolls into view and reports handled", async () => {
  const spy = vi.spyOn(Element.prototype, "scrollIntoView").mockImplementation(() => {});
  mockOverview({ awaiting: [pr(7)], mine: [] }); // the file's existing overview mock helper
  const handled = vi.fn();
  renderPanel({ focus: { repo: "web", id: 7 }, onFocusHandled: handled });
  const row = await screen.findByRole("button", { name: /PR 7/, expanded: true });
  expect(row).toBeInTheDocument();
  expect(spy).toHaveBeenCalled();
  expect(handled).toHaveBeenCalledTimes(1);
  spy.mockRestore();
});

test("a focused pull request that no listed group can hold says so", async () => {
  mockOverview({ awaiting: [], mine: [] });
  const handled = vi.fn();
  renderPanel({ focus: { repo: "billing", id: 99 }, onFocusHandled: handled });
  expect(await screen.findByText("Pull request !99 is not listed here. Track the billing repository to see it.")).toBeInTheDocument();
  expect(handled).toHaveBeenCalledTimes(1);
});
```
(`toast.info` is from sonner; the file's other tests show how toasts are asserted - if they mock sonner, assert on the mock instead of `findByText`. `renderPanel` gains an optional props argument.)

`src/App.test.tsx` - add one whole-app test next to the existing Work Manager tests: seed a notification with a `pr` target for the signed-in org, open the bell, click it, and assert `screen.getByRole("heading", { name: "Pull Requests" })` is shown and the Work Manager rail is active (whatever the file's existing `workMode` assertions check). Keep it to one test.

- [ ] **Step 2: Run them to verify they fail**

`npx vitest run src/lib/notifications.test.ts src/components/NotificationBell.test.tsx src/screens/WorkBoard.test.tsx src/screens/PrPanel.test.tsx --exclude "**/.claude/**"` - the new tests fail (unknown props, no target, no in-app path).

- [ ] **Step 3: Implement**

`src/lib/notifications.ts`: add `NotificationTarget`, the `target?` field with the doc comment `/** Where a click goes inside the app; absent on entries saved before this field existed, which fall back to href. */`, and set it in the three sources.

`src/components/NotificationBell.tsx`: props `{ org, onOpen }`; replace the title button block:
```tsx
{n.target && onOpen ? (
  <div className="mt-0.5 flex min-w-0 items-center gap-1">
    <button
      className="block min-w-0 flex-1 truncate text-left text-sm font-medium text-text hover:text-accent hover:underline"
      title="Open in the app"
      onClick={() => { setOpen(false); onOpen(n.target!); }}
    >
      {n.title}
    </button>
    {n.href && (
      <button
        aria-label={`Open in Azure DevOps: ${n.title}`}
        title="Open in Azure DevOps"
        className="shrink-0 rounded p-0.5 text-faint hover:text-accent"
        onClick={() => openUrl(n.href!).catch(() => toast.error("Could not open the browser."))}
      >
        <IconOpenExternal size={12} aria-hidden />
      </button>
    )}
  </div>
) : n.href ? (
  /* today's button, unchanged */
) : (
  /* today's plain div, unchanged */
)}
```
`ContextBar.tsx`: thread `onOpenNotification` → `<NotificationBell org={org} onOpen={onOpenNotification} />`.

`App.tsx`: the `workFocus` state, the handler, and the two props (Interfaces above). Also clear `workFocus` when `org` changes (add it to whatever effect resets per-org state, or a one-line effect), so a stale focus never lands in another organisation's board.

`WorkBoard.tsx`: props and the effect:
```ts
useEffect(() => {
  if (focusItem == null || !board.data) return;
  setOpenItem(focusItem);
  onFocusHandled?.();
}, [focusItem, board.data, onFocusHandled]);
```
`PrPanel.tsx`: props on `PrPanel`, `PrGroup`, `RepoPrSection`, `PrRow` (Interfaces above); the row `useRef` on its root `div`; the flash class `flash && "ring-2 ring-accent"` on the root; the panel-level "cannot hold it" effect and the 4000 ms timer. Keep the timer in a `useRef` and clear it in the effect cleanup.

- [ ] **Step 4: Run**

`npx vitest run src/lib/notifications.test.ts src/components/NotificationBell.test.tsx src/screens/WorkBoard.test.tsx src/screens/PrPanel.test.tsx src/components/ContextBar.test.tsx src/App.test.tsx src/ui-consistency.test.ts --exclude "**/.claude/**"` then `npx tsc --noEmit`. All green.

- [ ] **Step 5: Commit**

```bash
git add src/lib/notifications.ts src/lib/notifications.test.ts src/lib/actionIcons.ts src/components/NotificationBell.tsx src/components/NotificationBell.test.tsx src/components/ContextBar.tsx src/App.tsx src/App.test.tsx src/screens/WorkBoard.tsx src/screens/WorkBoard.test.tsx src/screens/PrPanel.tsx src/screens/PrPanel.test.tsx
git commit -q -F - <<'EOF'
feat(v2): a notification opens its work item or pull request inside the app

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 2: The review page: a spec citation scrolls the spec pane only

**Files:**
- Modify: `src-tauri/web/cases-specs.js:211-224` (`jumpTo`)
- Modify: `src-tauri/web/cases-page.css:315` (`.spec-doc`)
- Test: `src/lib/casesSpecs.test.ts` (loads the real script; add a DOM-driven block), `src-tauri/tests/import_parser.rs` (one CSS assertion)

**Interfaces:**
- Produces: `window.tcmSpecs.scrollWithin(container: Element, target: Element, margin: number): void` - a pure helper exported beside `splitCitation` etc.: `container.scrollTop += target.getBoundingClientRect().top - container.getBoundingClientRect().top - margin`. `jumpTo` calls `scrollWithin(article, h, 12)` instead of `h.scrollIntoView(...)`. Nothing else on the page changes.
- The CSS rule `.spec-doc` gains `overscroll-behavior: contain;` so a wheel that reaches the end of the spec does not carry on scrolling the test cases underneath.

- [ ] **Step 1: Write the failing tests**

`src/lib/casesSpecs.test.ts` - add:
```ts
describe("scrollWithin", () => {
  test("moves only the container, by the target's offset inside it less the margin", () => {
    const container = document.createElement("div");
    const target = document.createElement("h2");
    container.appendChild(target);
    let top = 100;
    Object.defineProperty(container, "scrollTop", { get: () => top, set: (v: number) => { top = v; }, configurable: true });
    container.getBoundingClientRect = () => ({ top: 40 } as DOMRect);
    target.getBoundingClientRect = () => ({ top: 400 } as DOMRect);
    const scrolled = vi.spyOn(window, "scrollTo").mockImplementation(() => {});
    const into = vi.spyOn(Element.prototype, "scrollIntoView").mockImplementation(() => {});
    (H as unknown as { scrollWithin: (c: Element, t: Element, m: number) => void }).scrollWithin(container, target, 12);
    expect(top).toBe(100 + 400 - 40 - 12);
    expect(scrolled).not.toHaveBeenCalled();
    expect(into).not.toHaveBeenCalled();
    scrolled.mockRestore();
    into.mockRestore();
  });
});
```
and a DOM test that a citation click goes through it and never through `scrollIntoView`: build `document.body.innerHTML` with a `.shell.with-specs` holding one `.rev` note whose text is `Spec: Rules.md 2 Login` and a `#tc-specs` section with one `.spec-tab` button and one `.spec-doc[data-spec=0]` article containing `<h2>2 Login</h2>`, plus the `<script type="application/json" id="tc-specs-data">[{"title":"Rules.md","kind":"file","source":"x"}]</script>` block the page emits; call `window.__tcmWireSpecs()`; spy `Element.prototype.scrollIntoView`; click the generated `a.spec-link`; assert the spy was not called and the article's `scrollTop` setter was hit (define it as above). Read `cases-specs.js` `wireSpecs`/`docsMeta` first to build a fixture it accepts; the existing `beforeAll` loads the script once, so put the DOM test after the pure ones and reset `document.body.innerHTML` in it.

`src-tauri/tests/import_parser.rs` - in `the_review_page_shows_a_spec_pane_only_when_given_documents` (line ~976) add `assert!(html.contains("overscroll-behavior: contain"))` next to the existing `.spec-doc` assertions (the CSS is inlined into the page's `<style>`; the file already asserts CSS substrings, e.g. `grid-template-rows: 0fr`).

- [ ] **Step 2: Run to verify failure**

`npx vitest run src/lib/casesSpecs.test.ts --exclude "**/.claude/**"` fails (`scrollWithin` undefined; `scrollIntoView` called). `cd src-tauri && CARGO_TARGET_DIR=target/gate cargo test --test import_parser the_review_page_shows_a_spec_pane` fails on the CSS assertion.

- [ ] **Step 3: Implement**

`cases-specs.js`: add before `root.tcmSpecs = {...}`:
```js
  // Scroll one container to a child, leaving every other scroll container
  // (the window above all) where it is. scrollIntoView would move them
  // all, and on this page that drags the test cases up under the spec.
  function scrollWithin(container, target, margin) {
    container.scrollTop += target.getBoundingClientRect().top - container.getBoundingClientRect().top - (margin || 0);
  }
```
export it in `root.tcmSpecs`, and in `jumpTo` replace `h.scrollIntoView({ block: 'start' });` with `scrollWithin(article, h, 12);`. `cases-page.css` `.spec-doc`: add `overscroll-behavior: contain;`.

- [ ] **Step 4: Run**

`npx vitest run src/lib/casesSpecs.test.ts src/lib/casesPageMarks.test.ts --exclude "**/.claude/**"`; `cd src-tauri && CARGO_TARGET_DIR=target/gate cargo test --test import_parser`. Green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/web/cases-specs.js src-tauri/web/cases-page.css src/lib/casesSpecs.test.ts src-tauri/tests/import_parser.rs
git commit -q -F - <<'EOF'
fix(v2): a spec citation on the review page scrolls the spec pane only, the test cases stay put

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 3: The review page: search one field

**Files:**
- Modify: `src-tauri/src/import_parser/html.rs:313-314` (search bar), `:365-368` (title), `:386-393` (tags), `:483-489` (steps)
- Modify: `src-tauri/web/cases-page.js:6-38` (`wireSearch`), `:249-283` (`swap`), and expose `window.__tcmWireSearch`
- Modify: `src-tauri/web/cases-page.css` (the `.searchbar select` rule next to `#tc-search`)
- Test: `src-tauri/tests/import_parser.rs` (`html_export_carries_cases_and_search`, line ~600), new `src/lib/casesPageFilter.test.ts`

**Interfaces:**
- Markup produced by `html.rs`:
  - In `.searchbar`, before `#tc-search`: `<select id='tc-field' aria-label='Search in'><option value='all'>All fields</option><option value='title'>Title</option><option value='id'>ID</option><option value='pre'>Prerequisites</option><option value='steps'>Steps</option><option value='tags'>Tags</option><option value='module'>Module</option></select>`. The input's placeholder becomes `Search test cases`.
  - The title text inside `<h2>` is wrapped: `<span class='title'>{esc(title)}</span>` (the `seq`/op/`wid` spans and `MARK_BUTTON` stay where they are).
  - Each tag chip is `<span class='chip tag'>`.
  - Step cells are `<td class='action'>` and `<td class='expected'>`.
- `cases-page.js` `wireSearch()` builds, per card, `fields = { all, title, id, pre, steps, tags, module }` (lower-cased text): `all` = `textContent`; `title` = `.title`; `id` = `data-key` plus the `.wid` text (so both `157957` and `#157957` match); `pre` = `.pre` text without the leading `Prerequisites:` label; `steps` = all `.action` and `.expected` cells joined with spaces; `tags` = all `.chip.tag` joined; `module` = `.chip.module`. `apply()` reads `document.getElementById('tc-field')`'s value (missing element = `all`) and matches every word against that field. The select's `change` event calls `apply()` (guarded by `dataset.wired` like the input). Escape clears the input only. `swap()` captures the select's value beside `q` and restores it before re-wiring. `window.__tcmWireSearch = function () { applyFilter = wireSearch(); }` is exposed for tests, next to `__tcmWireMarks`.
- CSS: `.searchbar select { font: inherit; padding: 6px 8px; border: 1px solid var(--border); border-radius: 6px; background: var(--surface); color: var(--text); }` using the page's own custom properties (read the `#tc-search` rule and copy its variable names exactly).

- [ ] **Step 1: Write the failing tests**

`src-tauri/tests/import_parser.rs`, in `html_export_carries_cases_and_search`, add:
```rust
assert!(html.contains("<select id='tc-field' aria-label='Search in'>"));
assert!(html.contains("<option value='title'>Title</option>"));
assert!(html.contains("<option value='pre'>Prerequisites</option>"));
assert!(html.contains("placeholder='Search test cases'"));
assert!(html.contains("<span class='title'>"));
assert!(html.contains("<td class='action'>"));
assert!(html.contains("<td class='expected'>"));
```
and, where that test's fixture has a tag, `assert!(html.contains("<span class='chip tag'>"))`. Check `the_review_page_folds_its_controls_into_one_menu` still passes (the select is outside the menu).

`src/lib/casesPageFilter.test.ts` (new; copy the loading pattern of `casesPageMarks.test.ts`: `readFileSync` the real `cases-page.js`, `new Function(src)()` once in `beforeAll`, reset `document.body.innerHTML` per test, then `window.__tcmWireSearch()`):
```ts
function page() {
  return `
<div class="page">
  <div class="searchbar">
    <select id="tc-field" aria-label="Search in">
      <option value="all">All fields</option><option value="title">Title</option><option value="id">ID</option>
      <option value="pre">Prerequisites</option><option value="steps">Steps</option><option value="tags">Tags</option><option value="module">Module</option>
    </select>
    <input id="tc-search" type="search"><span id="tc-count"></span>
  </div>
  <p id="tc-no-match" class="no-match hidden">No test cases match your search.</p>
  <div class="case" data-key="101"><h2><span class="seq">1</span><span class="wid">#101</span><span class="title">Login works</span></h2>
    <div class="metarow"><span class="metalabel">Module</span><span class="chip module">Auth</span></div>
    <div class="metarow"><span class="metalabel">Tags</span><span class="chip tag">smoke</span></div>
    <p class="pre"><b>Prerequisites:</b> A registered user</p>
    <table><tr><td class="num">1</td><td class="action">Open the login page</td><td class="expected">The form shows</td></tr></table></div>
  <div class="case" data-key="d1"><h2><span class="seq">2</span><span class="title">Password reset</span></h2>
    <div class="metarow"><span class="metalabel">Module</span><span class="chip module">Auth</span></div>
    <p class="pre"><b>Prerequisites:</b> <span class="none">None</span></p>
    <table><tr><td class="num">1</td><td class="action">Click Forgot password</td><td class="expected">A login email arrives</td></tr></table></div>
</div>`;
}
const shown = () => Array.from(document.querySelectorAll(".case")).filter((c) => !c.classList.contains("hidden")).map((c) => c.getAttribute("data-key"));
function search(field: string, text: string) {
  const sel = document.getElementById("tc-field") as HTMLSelectElement;
  sel.value = field; sel.dispatchEvent(new Event("change"));
  const input = document.getElementById("tc-search") as HTMLInputElement;
  input.value = text; input.dispatchEvent(new Event("input"));
}

test("all fields matches anywhere; a field narrows to that field", () => {
  search("all", "login");            expect(shown()).toEqual(["101", "d1"]); // title of one, a step of the other
  search("title", "login");          expect(shown()).toEqual(["101"]);
  search("steps", "login");          expect(shown()).toEqual(["d1"]);
  search("pre", "registered");       expect(shown()).toEqual(["101"]);
  search("tags", "smoke");           expect(shown()).toEqual(["101"]);
  search("module", "auth");          expect(shown()).toEqual(["101", "d1"]);
  search("id", "#101");              expect(shown()).toEqual(["101"]);
  search("id", "d1");                expect(shown()).toEqual(["d1"]);
});

test("the count and the no-match line follow the narrowed field", () => {
  search("title", "reset");
  expect(document.getElementById("tc-count")!.textContent).toBe("1 of 2 shown");
  search("tags", "reset");
  expect(shown()).toEqual([]);
  expect(document.getElementById("tc-no-match")!.classList.contains("hidden")).toBe(false);
});

test("Escape clears the text but keeps the field", () => {
  search("title", "reset");
  const input = document.getElementById("tc-search") as HTMLInputElement;
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  expect(input.value).toBe("");
  expect((document.getElementById("tc-field") as HTMLSelectElement).value).toBe("title");
  expect(shown()).toEqual(["101", "d1"]);
});

test("a page without the select searches every field", () => {
  document.getElementById("tc-field")!.remove();
  (window as unknown as { __tcmWireSearch: () => void }).__tcmWireSearch();
  const input = document.getElementById("tc-search") as HTMLInputElement;
  input.value = "login"; input.dispatchEvent(new Event("input"));
  expect(shown()).toEqual(["101", "d1"]);
});
```
The swap path: `swap()` is only reachable with the live-report globals; cover the capture with a unit of its own only if the implementer can call it cheaply (the existing marks test does not), otherwise state in the report that the swap restore was checked by reading the code and by the hand check below.

- [ ] **Step 2: Run to verify failure**

`npx vitest run src/lib/casesPageFilter.test.ts --exclude "**/.claude/**"` (fails: `__tcmWireSearch` undefined, then field logic); `cd src-tauri && CARGO_TARGET_DIR=target/gate cargo test --test import_parser html_export_carries_cases_and_search` fails.

- [ ] **Step 3: Implement** the markup (`html.rs`), the CSS, and `wireSearch`:
```js
  function fieldsOf(card) {
    var text = function (sel) {
      return Array.prototype.map.call(card.querySelectorAll(sel), function (e) { return e.textContent; }).join(' ');
    };
    var pre = card.querySelector('.pre');
    return {
      all: card.textContent.toLowerCase(),
      title: text('.title').toLowerCase(),
      id: ((card.getAttribute('data-key') || '') + ' ' + text('.wid')).toLowerCase(),
      pre: (pre ? pre.textContent.replace(/^\s*Prerequisites:\s*/, '') : '').toLowerCase(),
      steps: text('.action, .expected').toLowerCase(),
      tags: text('.chip.tag').toLowerCase(),
      module: text('.chip.module').toLowerCase()
    };
  }
```
`apply()`: `var field = document.getElementById('tc-field'); var key = field && field.value in fields[0] ? field.value : 'all';` (guard `fields.length`), then `fields[i][key].indexOf(w) !== -1`. Wire `change` on the select with its own `dataset.wired`. In `swap()`: `var sel = document.getElementById('tc-field'); var f = sel ? sel.value : '';` … after replace: `var sel2 = document.getElementById('tc-field'); if (sel2 && f) sel2.value = f;` before `applyFilter = wireSearch();`.

- [ ] **Step 4: Run**

`npx vitest run src/lib/casesPageFilter.test.ts src/lib/casesPageMarks.test.ts --exclude "**/.claude/**"`; `cd src-tauri && CARGO_TARGET_DIR=target/gate cargo test --test import_parser`. Green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/import_parser/html.rs src-tauri/tests/import_parser.rs src-tauri/web/cases-page.js src-tauri/web/cases-page.css src/lib/casesPageFilter.test.ts
git commit -q -F - <<'EOF'
feat(v2): the review page's search can be narrowed to the title, ID, prerequisites, steps, tags or module

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 4: Search Suites: use a requirement suite as the current PBI

**Files:**
- Modify: `src/screens/Suites.tsx:129-145` (props), `:333-345` (the `more` menu)
- Modify: `src/App.tsx:1139-1157` (`<Suites>` props)
- Test: `src/screens/Suites.test.tsx` (`renderSuites` helper at line 12), `src/App.test.tsx` (one test)

**Interfaces:**
- `Suites` gains `onSetCurrentPbi?: (pbi: { id: number; title: string }) => void`. For a row where `s.suite_type === "requirementTestSuite" && s.requirement_id` and `onSetCurrentPbi` is given, the `more` menu gains, first, `{ label: "Use as current PBI", onSelect: () => useAsPbi.mutate(s) }`. `useAsPbi` is a `useMutation`: `mutationFn: async (s: SuiteRef) => { const hits = await unwrap(commands.searchPbis(org, project, String(s.requirement_id))); return hits.find((h) => h.id === s.requirement_id) ?? { id: s.requirement_id!, title: s.name, work_item_type: "Product Backlog Item" }; }`, `onSuccess: (p) => { onSetCurrentPbi!({ id: p.id, title: p.title }); toast.success(`Current PBI: #${p.id} ${p.title}`); }`, `onError: (e) => toast.error(e.message)`. The row's `busy` flag includes `useAsPbi.isPending`.
- `App.tsx`: `onSetCurrentPbi={(p) => setPbiRaw({ id: p.id, title: p.title, work_item_type: "Product Backlog Item" })}` - no `goToSection`; the person stays on Search Suites and the chip in the context bar changes.

- [ ] **Step 1: Write the failing tests**

`src/screens/Suites.test.tsx`: extend `renderSuites` with a fourth optional parameter `onSetCurrentPbi?: (p: { id: number; title: string }) => void` passed through. Add, modelled on "Manage hands the suite to Suite Management" (lines 76-104; it clicks `More actions for PBI 42 suite` and then a `menuitem`):
```ts
const REQUIREMENT_SUITE_PLANS = [
  {
    plan: PLAN,
    suites: [{ id: 91, name: "PBI 42 suite", suite_type: "requirementTestSuite", requirement_id: 42, parent_id: null }],
  },
];

test("Use as current PBI resolves the PBI's own title and hands it up without leaving the screen", async () => {
  const queries: string[] = [];
  baseMock((cmd, args) => {
    if (cmd === "list_plans_with_suites") return REQUIREMENT_SUITE_PLANS;
    if (cmd === "search_pbis") {
      queries.push((args as { query: string }).query);
      return [{ id: 42, title: "Real PBI title", work_item_type: "Product Backlog Item" }];
    }
  });
  const picked: unknown[] = [];
  renderSuites(undefined, undefined, undefined, (p) => picked.push(p));
  await screen.findByText("PBI 42 suite");
  fireEvent.click(screen.getByRole("button", { name: "More actions for PBI 42 suite" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Use as current PBI" }));
  await waitFor(() => expect(picked).toEqual([{ id: 42, title: "Real PBI title" }]));
  expect(queries).toEqual(["42"]);
  expect(screen.getByRole("button", { name: "More actions for PBI 42 suite" })).toBeInTheDocument(); // still here
});

test("when the lookup finds nothing the suite's own name stands in", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites") return REQUIREMENT_SUITE_PLANS;
    if (cmd === "search_pbis") return [];
  });
  const picked: unknown[] = [];
  renderSuites(undefined, undefined, undefined, (p) => picked.push(p));
  await screen.findByText("PBI 42 suite");
  fireEvent.click(screen.getByRole("button", { name: "More actions for PBI 42 suite" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Use as current PBI" }));
  await waitFor(() => expect(picked).toEqual([{ id: 42, title: "PBI 42 suite" }]));
});

test("a static suite offers no Use as current PBI", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites")
      return [{ plan: PLAN, suites: [{ id: 93, name: "Sprint stories", suite_type: "staticTestSuite", requirement_id: null, parent_id: null }] }];
  });
  renderSuites(() => {}, undefined, () => {}, () => {});
  const row = (await screen.findByText("Sprint stories")).closest("button")!;
  fireEvent.mouseEnter(within(row).getByRole("button", { name: "More actions for Sprint stories" }));
  const menu = screen.getByRole("menu", { name: "More actions for Sprint stories" });
  expect(within(menu).getAllByRole("menuitem").map((m) => m.textContent)).toEqual(["Manage", "Run Tests", "Report"]);
});
```
(`baseMock` and `PLAN` are the file's existing helpers; `search_pbis` receives `{ organization, project, query }` per `src/bindings.ts:22`. The existing test "a row shows View and Edit cases, with Manage, Run Tests and Report behind More" does not pass `onSetCurrentPbi`, so its `["Manage", "Run Tests", "Report"]` assertion stays as it is.)

`src/App.test.tsx`: one test beside "the tour gives the Search Suites handoff back" (line ~944) or the `Ctrl+7` test: open Search Suites, choose "Use as current PBI" on a requirement suite (mock `list_plans_with_suites` and `search_pbis`), and assert the context bar's PBI chip shows the resolved title while the Search Suites screen is still rendered.

- [ ] **Step 2: Run to verify failure**

`npx vitest run src/screens/Suites.test.tsx --exclude "**/.claude/**"` fails (no menu item).

- [ ] **Step 3: Implement** per Interfaces. Put the new entry first in `more` only for requirement suites; the existing comment at line ~420 ("Manage, Run Tests and Report are the extra options") gains "Use as current PBI".

- [ ] **Step 4: Run**

`npx vitest run src/screens/Suites.test.tsx src/App.test.tsx src/ui-consistency.test.ts --exclude "**/.claude/**"`; `npx tsc --noEmit`. Green.

- [ ] **Step 5: Commit**

```bash
git add src/screens/Suites.tsx src/screens/Suites.test.tsx src/App.tsx src/App.test.tsx
git commit -q -F - <<'EOF'
feat(v2): Search Suites can make a requirement suite's PBI the current one without leaving the screen

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

## After execution

Hand checks, in a dev build: (1) let the assigned-work poll or a PR refresh raise a notification; clicking it lands on the Board with the item's drawer open, or on Pull Requests with the row expanded and ringed; the small external-link button still opens Azure DevOps. (2) View in Browser with a linked wiki or file: click a "Spec:" citation; the spec pane scrolls to the heading and the "Test Cases" title does not move; wheel to the end of the spec and the page underneath stays put. (3) On the same page, pick "Title" and type a word that occurs only in a step: no match; pick "Steps": it shows; with a live report (from the app), edit a case so the page swaps and confirm the chosen field survives. (4) Search Suites: "Use as current PBI" on a requirement suite changes the chip at the top to the PBI's real title and the screen stays.

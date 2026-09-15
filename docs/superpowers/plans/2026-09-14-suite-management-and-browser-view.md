# Suite Management, Browser View and Permissions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Six independent improvements: a hide control for findings in the browser view, cached suite cases so Suite Management stops reloading, a searchable suite picker, sticky Apply order / Reset buttons, a New test suite button that disappears when Azure DevOps says the user may not create one, and "Suite Lookup" renamed to "Search Suites" with a Manage chip that opens a suite in Suite Management.

**Architecture:** Five of the six are frontend-only and each touches one area. The permission gate adds a Rust security-evaluation call (the same `permissionevaluationbatch` endpoint the delete gate already uses), a new command, and a query that hides one button.

**Tech Stack:** Rust (reqwest, serde, wiremock), React 19 + TypeScript, TanStack React Query, Tailwind tokens, vitest (jsdom), plain CSS/JS for the browser-view page under `src-tauri/web/`.

**Spec:** No separate design doc. The five requests and the two decisions below, agreed with the user on 2026-09-14, are the spec.

## Decisions (the spec)

1. **Findings in the browser view get a hide control, not removal.** Mirror the reviewer-notes pattern exactly: an × on each findings block, plus one sticky-bar button that hides or shows them all, remembered per reader. The block stays open by default.
2. **The New test suite button hides only on an explicit "no".** If the permission check cannot be made (offline, odd response, an org that answers strangely), the button stays and the existing refusal handling remains the backstop. A failed check must never take away a button the user is entitled to. This is the opposite posture to the delete gate, which fails closed, and the code must say why.
3. **Suite Management caches a suite's cases** the way the plan tree already is: served from disk instantly, refreshed in the background after 5 minutes. The screen's own mutations (reorder, copy in, new suite) already invalidate their keys, so a user's own changes still show at once.
4. **The suite picker becomes the searchable combobox** (`ui/combobox.tsx`) rather than a wider `ui/select.tsx`, because a project's suite list runs to hundreds.
5. **Apply order / Reset also appear in a sticky bar at the bottom right**, only while that suite's order is actually changed. The inline row stays where it is.
6. **"Suite Lookup" becomes "Search Suites"** everywhere it is shown, and each suite row gains a Manage chip that opens Suite Management on that suite. Past changelog entries keep the old name — they record what shipped. The internal section id stays `suites`, so saved preferences and the tour keep working.

## Global Constraints

- **No DELETE calls** to Azure DevOps anywhere. The ADO client issues GET / POST / PATCH only.
- **Every Rust test is an integration test under `src-tauri/tests/`.** Never a `#[cfg(test)]` module inside `src/`.
- **`src/bindings.ts` is generated** by `cd src-tauri && cargo test --test bindings`. Never hand-edit it. Task 5 adds a command, so it must be regenerated and committed; every other task must leave it untouched. If it shows as modified with identical content, that is a line-ending artifact: `git checkout -- src/bindings.ts`.
- **Colours come from theme tokens** (`text-text`, `text-muted`, `text-faint`, `bg-surface`, `bg-surface-2`, `border-border`, `text-danger`, `accent`…). Never hardcode a colour; `src/ui-consistency.test.ts` is the gate and must never be weakened. In `src-tauri/web/*.css`, use the page's CSS variables (`var(--accent)`, `var(--warning)`, `var(--text)`, `var(--faint)`, `var(--danger)`, `var(--surface)`, `var(--bg)`).
- **Icons come from `src/lib/actionIcons.ts`**, named for what the button does.
- **Caching goes through `src/lib/cache.ts`** — `persistentQuery` with a `cacheKeys` key and a `CACHE` preset. `src/lib/cache.test.ts` fails on a hand-written key or a raw TTL.
- **User-facing errors name no URL.** Return one of the sentences in `ado/transport.rs` via `network_error`; log the raw error with `applog`.
- **Don't run `npx prettier`.** Match the surrounding style by hand.
- **The machine is shared with the user.** Run one build or test command at a time and wait for it. Long runs: use `run_in_background` and wait for the notification rather than polling.
- `src/App.test.tsx` has a documented load-induced flake: one failure that passes on a re-run is that flake; two different failures are not.
- **Commit with a Bash heredoc** — `git commit -F - <<'MSG' … MSG` (pick a delimiter that cannot collide). End every message with `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`. Confirm with `git log -1`.
- Work on a branch off `main`; do not merge, push or release. The user decides that separately.
- All commands run from the repository root unless a step says `cd src-tauri`.

---

## File Structure

**Task 1 — findings hide control (browser view)**
- Modify `src-tauri/src/import_parser/html.rs`: wrap the findings block, add its × and the sticky-bar toggle.
- Modify `src-tauri/web/cases-page.css`: the collapse rules and the × styling, mirroring `.rev-wrap` / `.rev-close`.
- Modify `src-tauri/web/cases-page.js`: generalise the notes toggle so it drives both blocks.
- Modify `src-tauri/tests/import_parser.rs`: a test beside the notes-toggle one.

**Task 2 — cached suite cases**
- Modify `src/lib/cache.ts`: one new `cacheKeys` entry.
- Modify `src/lib/cache.test.ts`: pin the new key string.
- Modify `src/screens/ManageCases/SuiteCases.tsx`: use `persistentQuery`.

**Task 3 — searchable suite picker**
- Modify `src/components/ui/combobox.tsx`: accept value/label pairs and expose listbox roles.
- Modify `src/screens/ManageCases/PlanTable.tsx`: use it for "Copy to".
- Modify `src/screens/ManageCases/suites.test.tsx`: the picker's test.

**Task 4 — sticky Apply order / Reset**
- Create `src/screens/ManageCases/dirtySuites.ts`: a module-scope store of which suites have unsaved order, so two sticky bars never overlap.
- Create `src/screens/ManageCases/dirtySuites.test.ts`.
- Modify `src/screens/ManageCases/SuiteCases.tsx`: register dirtiness, render the sticky bar.
- Modify `src/screens/ManageCases/SuiteCases.test.tsx`: cover the sticky bar.

**Task 5 — New test suite permission gate**
- Create `src-tauri/src/ado/permissions.rs`: the shared batch evaluation plus `may_manage_test_suites`.
- Modify `src-tauri/src/ado/mod.rs`: declare the module.
- Modify `src-tauri/src/ado/deletion.rs`: take the namespace/permission constants from the new module instead of keeping its own copies.
- Modify `src-tauri/src/commands/testplan.rs` and `src-tauri/src/lib.rs`: the new command.
- Create `src-tauri/tests/permissions.rs`.
- Modify `src/screens/ManageCases/PlanTable.tsx`: hide the button on an explicit no.
- Modify `src/screens/ManageCases/suites.test.tsx` (or add `permissions.test.tsx` in the same folder): cover shown / hidden / unknown.
- Regenerate `src/bindings.ts`.

---

### Task 1: A hide control for findings in the browser view

**Files:**
- Modify: `src-tauri/src/import_parser/html.rs` (the findings block, around line 294; the search bar, around line 175)
- Modify: `src-tauri/web/cases-page.css` (beside the `.rev-wrap` rules, around line 86, and the `.findings` rules, around line 155)
- Modify: `src-tauri/web/cases-page.js` (`wireNotesToggle`, around line 41, and the delegated close handler, around line 84)
- Test: `src-tauri/tests/import_parser.rs` (beside `notes-toggle`)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: nothing other tasks consume. The page markup gains `find-wrap`, `find-close`, `body.findings-off`, `find-closed` and the button `id='tc-findings'`.

**Context:** the findings block has no JavaScript today — it is a native `<details open>` with a `<summary>`, so the disclosure arrow is its only control. `export_queue_to_html` is the single generator behind all three browser views (existing cases from Suite Lookup / View Test Cases, the draft queue from the Import screen, and the save-to-disk export), so this one change covers them all. The execution report (`src-tauri/src/report.rs`) has no findings and is untouched.

- [ ] **Step 1: Write the failing test**

Find the existing test in `src-tauri/tests/import_parser.rs` that asserts `id='tc-notes'` (search for `notes-toggle.html`) and add this one directly after it. It mirrors that test's structure, because the feature mirrors that feature:

```rust
/// Findings get the same two controls the reviewer notes have: one button
/// in the sticky bar for all of them, and an x per case. A page of twenty
/// findings blocks is otherwise a page nobody can read past.
#[test]
fn findings_can_be_hidden_from_the_page() {
    use v2_lib::model::CaseFinding;
    let with_findings = vec![TestCase {
        title: "Has findings".into(),
        steps: vec![Step { action: "Do".into(), expected: "Done".into() }],
        automation_status: "Not Automated".into(),
        findings: vec![CaseFinding {
            kind: "spec".into(),
            subject: "AC-4".into(),
            detail: "The spec does not say what happens on a second submit.".into(),
        }],
        ..Default::default()
    }];
    let path = tmp_path("findings-toggle.html");
    export_queue_to_html(&with_findings, &path, "", None, &Default::default()).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();

    // The toggle belongs in the sticky bar, beside the notes one.
    let bar = html.split("class='searchbar'").nth(1).expect("search bar");
    let bar = bar.split("</div>").next().unwrap();
    assert!(bar.contains("id='tc-findings'"), "toggle belongs in the sticky bar: {bar}");
    // One class on <body> does the hiding, as an animated grid collapse.
    assert!(html.contains("body.findings-off .find-wrap"), "{html}");
    // And each block carries its own named x.
    assert!(html.contains("class='find-close'"), "{html}");
    assert!(html.contains("find-closed"), "{html}");
    assert!(
        html.contains("aria-label='Hide these findings'"),
        "the x needs a name for screen readers: {html}"
    );
    // Still open by default: a finding behind a closed disclosure is a
    // finding nobody reads.
    assert!(html.contains("<details class='findings' open>"), "{html}");

    // No findings anywhere: no button.
    let without = vec![TestCase {
        title: "No findings".into(),
        steps: vec![Step { action: "Do".into(), expected: "Done".into() }],
        automation_status: "Not Automated".into(),
        ..Default::default()
    }];
    let path2 = tmp_path("findings-toggle-none.html");
    export_queue_to_html(&without, &path2, "", None, &Default::default()).unwrap();
    let plain = std::fs::read_to_string(&path2).unwrap();
    assert!(!plain.contains("id='tc-findings'"), "nothing to hide, so no button");
}
```

Check `CaseFinding`'s real field names in `src-tauri/src/model.rs` before running, and use them; the three above are the expected shape but the struct is the authority.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --test import_parser findings_can_be_hidden`
Expected: FAIL — the assertion on `id='tc-findings'` does not match (or a compile error if `CaseFinding`'s fields differ, which you fix per Step 1).

- [ ] **Step 3: Emit the markup**

In `src-tauri/src/import_parser/html.rs`, the sticky bar currently adds the notes button only when some case has notes. Add the findings button the same way, immediately after it:

```rust
        if queue.iter().any(|tc| !tc.findings.is_empty()) {
            "<button id='tc-findings' type='button' aria-pressed='false'>Hide findings</button>"
                .to_string()
        } else {
            String::new()
        },
```

(Match the surrounding expression style: the notes entry is an `if/else` returning a `String` inside the same `vec!`/`parts` list. Put the new entry directly after it and keep the comment style.)

Then wrap the findings block. Replace:

```rust
            parts.push(format!(
                "<details class='findings' open><summary>Findings ({})</summary>",
                tc.findings.len()
            ));
```

with:

```rust
            parts.push(format!(
                "<div class='find-wrap'><details class='findings' open><summary>Findings ({})\
                 <button type='button' class='find-close' aria-label='Hide these findings' \
                 title='Hide these findings'>&#215;</button></summary>",
                tc.findings.len()
            ));
```

Find where that block is closed (the matching `</details>` push after the findings loop) and close the wrapper too: `</details></div>`.

- [ ] **Step 4: Style it**

In `src-tauri/web/cases-page.css`, directly after the `.rev-wrap` collapse rules, add the findings equivalents:

```css
/* Findings hide the same way the reviewer notes do - an animated grid-row
   collapse, so a hidden block leaves the tab order instead of blinking out.
   See the .rev-wrap rules above for why fr units rather than display:none. */
.find-wrap { display: grid; grid-template-rows: 1fr;
             transition: grid-template-rows .28s ease, opacity .22s ease,
                         margin .28s ease, visibility 0s linear .28s; }
body.findings-off .find-wrap, .find-wrap.find-closed {
  grid-template-rows: 0fr; opacity: 0; margin: 0; visibility: hidden; }
body:not(.findings-off) .find-wrap:not(.find-closed) { transition-delay: 0s; }
.find-wrap > .findings { overflow: hidden; min-height: 0; }
```

And beside the existing `.findings > summary` rules, the × itself — same quiet-until-hover treatment as `.rev-close`, in the findings' warning colour:

```css
.findings > summary { display: flex; align-items: center; gap: 4px; }
.find-close { margin-left: auto; font: inherit; font-size: 14px; line-height: 1;
              cursor: pointer; border: 0; border-radius: 4px; padding: 1px 6px;
              color: var(--faint); background: none; opacity: 0;
              transition: opacity .15s ease; }
.findings > summary:hover .find-close, .find-close:focus-visible { opacity: 1; }
.find-close:hover { color: var(--danger); background: color-mix(in srgb, var(--danger) 12%, transparent); }
@media print { .find-close { display: none; } }
```

Keep the existing `.findings > summary` declaration and add the flex line to it rather than duplicating the whole rule if that reads better in place.

- [ ] **Step 5: Wire the buttons**

In `src-tauri/web/cases-page.js`, `wireNotesToggle` hardcodes the notes ids. Generalise it into one function used twice, keeping every comment that explains WHY (the `file://` storage caveat, and why Show resurrects individually closed blocks):

```js
  // --- Reviewer notes / findings on/off. One class on <body>; the CSS does
  // the rest. Both blocks work the same way, so they share this.
  function wireBlockToggle(opts) {
    var btn = document.getElementById(opts.buttonId);
    if (!btn) return;
    // The page is opened from a temp file, and a file:// origin can refuse
    // storage outright - so the preference is best-effort and the button
    // still works without it.
    function remember(off) {
      try { localStorage.setItem(opts.key, off ? '1' : '0'); } catch (e) {}
    }
    function recall() {
      try { return localStorage.getItem(opts.key) === '1'; } catch (e) { return false; }
    }
    function paint(off) {
      document.body.classList.toggle(opts.bodyClass, off);
      btn.setAttribute('aria-pressed', off ? 'true' : 'false');
      btn.textContent = off ? opts.showLabel : opts.hideLabel;
    }
    paint(recall());
    if (!btn.dataset.wired) {
      btn.dataset.wired = '1';
      btn.addEventListener('click', function () {
        var off = !document.body.classList.contains(opts.bodyClass);
        paint(off);
        remember(off);
        // Show brings EVERY block back, including ones closed one-by-one
        // with their own x - one button that undoes everything, rather
        // than the reader having to remember which x they clicked where.
        if (!off) {
          var closed = document.querySelectorAll(opts.closedSelector);
          for (var i = 0; i < closed.length; i++) closed[i].classList.remove(opts.closedClass);
        }
      });
    }
  }

  function wireNotesToggle() {
    wireBlockToggle({
      buttonId: 'tc-notes',
      key: 'tcm-report-notes-off',
      bodyClass: 'notes-off',
      closedSelector: '.rev-wrap.rev-closed',
      closedClass: 'rev-closed',
      hideLabel: 'Hide reviewer notes',
      showLabel: 'Show reviewer notes',
    });
  }

  function wireFindingsToggle() {
    wireBlockToggle({
      buttonId: 'tc-findings',
      key: 'tcm-report-findings-off',
      bodyClass: 'findings-off',
      closedSelector: '.find-wrap.find-closed',
      closedClass: 'find-closed',
      hideLabel: 'Hide findings',
      showLabel: 'Show findings',
    });
  }
```

Call `wireFindingsToggle();` next to the existing `wireNotesToggle();`. Then extend the delegated per-block × handler to cover both, keeping its comments:

```js
  document.addEventListener('click', function (e) {
    var btn = e.target && e.target.closest ? e.target.closest('.rev-close, .find-close') : null;
    if (!btn) return;
    // The x lives inside a <summary>; without this, hiding the block would
    // also toggle the disclosure underneath it.
    e.preventDefault();
    e.stopPropagation();
    var wrap = btn.closest('.rev-wrap, .find-wrap');
    if (!wrap) return;
    wrap.classList.add(wrap.classList.contains('find-wrap') ? 'find-closed' : 'rev-closed');
  });
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --test import_parser`
Expected: PASS, including two existing tests that must not change behaviour: the notes-toggle one (the JS generalisation) and `a_cases_findings_render_in_their_own_block`, which asserts exactly one `<details class='findings'` block and its `Findings (2)` summary — the new wrapper `<div>` sits outside that, so it still holds.

- [ ] **Step 7: Look at the page**

The JS and CSS have no automated coverage — the Rust test asserts the markup only. Export a page that has both notes and findings and open it:

Run: `cd src-tauri && cargo test --test import_parser findings_can_be_hidden -- --nocapture`, then open the generated `findings-toggle.html` from the temp directory the test used (`tmp_path` in that test file says where).
Check: both buttons appear; each hides only its own blocks; the × hides one block; Show brings it back; the choice survives a reload.
If you cannot open a browser in this environment, say so in your report rather than claiming it works.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/import_parser/html.rs src-tauri/web/cases-page.css src-tauri/web/cases-page.js src-tauri/tests/import_parser.rs
git commit -F - <<'MSG'
feat(v2): findings can be hidden in the browser view

Findings now carry the same two controls the reviewer notes have: one
button in the sticky bar that hides or shows every block (remembered per
reader), and an x per case that collapses just that one. The toggle
logic is now one function driving both blocks rather than two copies.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
MSG
git log -1 --stat
```

---

### Task 2: Suite Management serves a suite's cases from the cache

**Files:**
- Modify: `src/lib/cache.ts` (`cacheKeys`)
- Modify: `src/lib/cache.test.ts` (the key-pinning test)
- Modify: `src/screens/ManageCases/SuiteCases.tsx:40-45`
- Test: `src/screens/ManageCases/SuiteCases.test.tsx`

**Interfaces:**
- Consumes: `persistentQuery`, `CACHE`, `cacheKeys` from `src/lib/cache.ts`.
- Produces: `cacheKeys.suiteCases(org, project, planId, suiteId) => "suite-cases:{org}/{project}/{planId}/{suiteId}"`.

- [ ] **Step 1: Write the failing tests**

In `src/lib/cache.test.ts`, add to the key-pinning test ("cache keys are the strings earlier versions stored"):

```ts
  // New in this release, so not an earlier version's string - but pinned
  // from now on, since changing it throws away every reader's copy.
  expect(cacheKeys.suiteCases("acme", "Web", 9, 91)).toBe("suite-cases:acme/Web/9/91");
```

In `src/screens/ManageCases/SuiteCases.test.tsx`, add a test that the second visit to a suite paints from disk without asking Azure DevOps again. Read that file first and follow its existing mount/mock helpers (`testSupport.tsx` in the same folder); the shape is:

```tsx
test("a suite already read once paints from the cache instead of loading again", async () => {
  // First visit: the two reads happen and the rows appear.
  const first = mountSuite();
  expect(await screen.findByText(/Case 201|#201/)).toBeInTheDocument();
  const readsFirst = first.calls.filter((c) => c.cmd === "list_suite_entries").length;
  expect(readsFirst).toBe(1);
  first.unmount();

  // Second visit, same suite: rows are there in the first paint, with no
  // "Loading test cases" and no fresh read.
  const second = mountSuite();
  expect(screen.getByText(/Case 201|#201/)).toBeInTheDocument();
  expect(screen.queryByText("Loading test cases")).not.toBeInTheDocument();
  expect(second.calls.filter((c) => c.cmd === "list_suite_entries")).toHaveLength(0);
});
```

Adapt the row matcher to what the list actually renders, and the mount helper to the file's own (it may need a fresh React Query client per mount so the second visit proves the *disk* cache, not React Query's memory). If the existing helper shares one client, create a second helper that mounts with a new client and say so in your report.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/lib/cache.test.ts src/screens/ManageCases/SuiteCases.test.tsx`
Expected: FAIL — `cacheKeys.suiteCases is not a function`, and the second-visit test sees a fresh read.

- [ ] **Step 3: Add the key**

In `src/lib/cache.ts`, add to `cacheKeys` after `plansSuites`:

```ts
  suiteCases: (org: string, project: string, planId: number, suiteId: number) =>
    `suite-cases:${org}/${project}/${planId}/${suiteId}`,
```

- [ ] **Step 4: Use the persistent cache**

In `src/screens/ManageCases/SuiteCases.tsx`, add the import and replace the query. Before:

```tsx
  const cases = useQuery({
    queryKey: key,
    queryFn: () => loadSuiteCases(org, project, planId, suiteId),
    retry: false,
  });
```

After:

```tsx
  // Reopening a suite must not re-read it: two Azure DevOps calls per
  // suite made Suite Management feel like it was loading from scratch
  // every time, even standing still on one PBI. Served from disk at once,
  // refreshed in the background after five minutes - and this screen's own
  // edits (reorder, copy in, new suite) invalidate the key, so the user's
  // own changes never wait for that.
  const cases = useQuery({
    queryKey: key,
    ...persistentQuery({
      key: cacheKeys.suiteCases(org, project, planId, suiteId),
      fetcher: () => loadSuiteCases(org, project, planId, suiteId),
      ...CACHE.structure,
      staleMs: 5 * 60_000,
    }),
    retry: false,
  });
```

Import line: `import { CACHE, cacheKeys, persistentQuery } from "../../lib/cache";`

- [ ] **Step 5: Run the tests to verify they pass**

Run: `npx vitest run src/lib/cache.test.ts src/screens/ManageCases`
Expected: PASS. If a sibling test in that folder now fails because rows appear without its `await`, that is the intended behaviour arriving earlier — update that test's expectation and name it in your report. Anything else is a regression.

- [ ] **Step 6: Full suite and typecheck**

Run: `npx tsc --noEmit`
Expected: no errors.

Run: `npx vitest run`
Expected: every file passes.

- [ ] **Step 7: Commit**

```bash
git add src/lib/cache.ts src/lib/cache.test.ts src/screens/ManageCases
git commit -F - <<'MSG'
perf(v2): Suite Management serves a suite's cases from the cache

Opening a suite read its entries and its test points every single time,
so moving around Suite Management - even on one PBI - always looked like
a fresh load. The cases now come from the local cache instantly and
refresh in the background after five minutes; the screen's own edits
still invalidate the key, so your own changes show at once.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
MSG
git log -1 --stat
```

---

### Task 3: The suite picker is bigger and searchable

**Files:**
- Modify: `src/components/ui/combobox.tsx`
- Modify: `src/screens/ManageCases/PlanTable.tsx:128-140` (the `Select` for "Copy to")
- Test: `src/screens/ManageCases/suites.test.tsx` (the "Copy to" test, around line 25-50)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `Combobox` gains two optional props —
  - `items?: Array<{ value: string; label: string }>` — value/label pairs; when given, `options` is ignored, search matches the label, and `onChange` receives the `value`.
  - `triggerClassName?: string` — extra classes for the trigger button (the picker needs a minimum width).
  The panel's rows also gain listbox/option roles.

- [ ] **Step 1: Write the failing test**

In `src/screens/ManageCases/suites.test.tsx`, the "Copy to" test currently opens the select and asserts the full option list. Replace that part with one that types to narrow. Keep the rest of the test as it is:

```tsx
  // A target is needed: static suites of this plan (root included), never
  // the PBI suite. The list is searchable - a project's suites run long.
  await openSelect(auth, "Copy to");
  const labels = screen.getAllByRole("option").map((o) => o.textContent?.trim());
  expect(labels).toEqual(["Plan root", "Regression", "Smoke"]);
  fireEvent.change(screen.getByPlaceholderText("Search…"), { target: { value: "regr" } });
  expect(screen.getAllByRole("option").map((o) => o.textContent?.trim())).toEqual(["Regression"]);
  fireEvent.click(screen.getByRole("option", { name: "Regression" }));
```

Note two deliberate changes to what the old assertion said: the placeholder "Pick a suite" is no longer a row in the list (the combobox shows it on the trigger instead, and carries its own Clear), and the indentation that `indented()` adds is whitespace this assertion trims. If the nesting must remain visible, keep `indented()` in the label and trim in the test, as above.

- [ ] **Step 2: Run the test to verify it fails**

Run: `npx vitest run src/screens/ManageCases/suites.test.tsx`
Expected: FAIL — there is no "Search…" box, and the first assertion still sees "Pick a suite" in the list.

- [ ] **Step 3: Teach the combobox value/label pairs and listbox roles**

In `src/components/ui/combobox.tsx`:

Add to the props type (keep the existing doc comment and extend it to say the searchable list takes either plain strings or value/label pairs):

```ts
  /** Value/label pairs, for lists whose labels are not their values (ids,
   * indented names). When given, `options` is ignored: search matches the
   * label, and `onChange` receives the value. */
  items?: Array<{ value: string; label: string }>;
  /** Extra classes for the trigger button (width, padding). */
  triggerClassName?: string;
```

Normalise both inputs to one list, and look the selected label up for the trigger:

```ts
  const rows = useMemo(
    () => items ?? options.map((o) => ({ value: o, label: o })),
    [items, options],
  );
  const filtered = useMemo(
    () => (q ? rows.filter((r) => r.label.toLowerCase().includes(q)) : rows),
    [rows, q],
  );
  const showCustom = allowCustom && query.trim() && !rows.some((r) => r.label.toLowerCase() === q);
  const selectedLabel = rows.find((r) => r.value === value)?.label ?? value;
```

Then update the render to use `rows` / `filtered` / `selectedLabel`:
- the trigger's span shows `selectedLabel || placeholder`;
- `commit(filtered[active].value)` on Enter, and `commit(r.value)` on click;
- each row's key is `r.value`, its text is `r.label`, `details?.[r.label]` keeps working for the existing callers, and the check mark shows when `r.value === value`;
- add `className={cn("…", triggerClassName)}` to the trigger button.

Give the panel proper roles, so the list is a listbox to assistive tech and to tests:
- trigger: `role="combobox"`, `aria-expanded={open}`, `aria-haspopup="listbox"`;
- `<ul>`: `role="listbox"`;
- each row's `<button>`: `role="option"` with `aria-selected={r.value === value}`.

Check the trigger does not already carry `role="combobox"` implicitly through something else in the file; if `ui/select.tsx` sets the same roles, copy its exact attribute set so the two dropdowns look identical to a test.

- [ ] **Step 4: Use it for the suite picker**

In `src/screens/ManageCases/PlanTable.tsx`, replace the `Select` block:

```tsx
        <Combobox
          ariaLabel="Copy to"
          className="min-w-56"
          triggerClassName="py-1"
          placeholder="Pick a suite"
          value={target}
          onChange={setTarget}
          items={staticTargets.map((t) => ({ value: String(t.id), label: t.label }))}
        />
```

Replace the `Select` import with `import Combobox from "../../components/ui/combobox";` (check the file's default/named export shape first). The picker was previously disabled when `busy || staticTargets.length === 0`; the combobox has no `disabled` prop, so keep the behaviour by rendering it only when `staticTargets.length > 0` and leaving the Copy to suite button's own `disabled` as the guard against a click during `busy`. If you would rather add a `disabled` prop to the combobox, that is fine too — but then wire it to the trigger and say so in your report.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `npx vitest run src/screens/ManageCases`
Expected: PASS.

Run: `npx vitest run src/components src/screens/CreateWorkItem.test.tsx`
Expected: PASS — the combobox's other call sites still behave (it is used for iteration/area pickers; grep for `combobox` to find which tests cover them and run those too).

- [ ] **Step 6: Typecheck and full suite**

Run: `npx tsc --noEmit`
Expected: no errors.

Run: `npx vitest run`
Expected: every file passes, `src/ui-consistency.test.ts` included.

- [ ] **Step 7: Commit**

```bash
git add src/components/ui/combobox.tsx src/screens/ManageCases
git commit -F - <<'MSG'
feat(v2): the Copy to suite picker is searchable and wider

A project's suites run to hundreds, and the picker was the short fixed
list dropdown: no search, and a trigger too narrow for a real suite
name. It is now the searchable combobox, which also learned value/label
pairs (suite ids with indented names) and proper listbox roles.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
MSG
git log -1 --stat
```

---

### Task 4: Apply order and Reset stick to the bottom right

**Files:**
- Create: `src/screens/ManageCases/dirtySuites.ts`
- Create: `src/screens/ManageCases/dirtySuites.test.ts`
- Modify: `src/screens/ManageCases/SuiteCases.tsx` (the button row, around line 104, and the `dirty` computation above it)
- Test: `src/screens/ManageCases/SuiteCases.test.tsx`

**Interfaces:**
- Consumes: nothing from other tasks (Task 2 touches the same file's query; if both are done, rebase carefully — they do not overlap in lines).
- Produces:
  - `markSuiteDirty(suiteId: number, dirty: boolean): void`
  - `useDirtyRank(suiteId: number): number | null` — 0 for the first suite that became dirty, 1 for the next, `null` when this suite is clean. The sticky bar uses it as its stacking offset so two dirty suites never sit on top of each other.

- [ ] **Step 1: Write the failing tests**

Create `src/screens/ManageCases/dirtySuites.test.ts`:

```ts
import { afterEach, expect, test } from "vitest";
import { dirtyRanks, markSuiteDirty } from "./dirtySuites";

afterEach(() => {
  for (const id of [...dirtyRanks().keys()]) markSuiteDirty(id, false);
});

test("ranks are the order suites became dirty, and close up when one is saved", () => {
  markSuiteDirty(91, true);
  markSuiteDirty(92, true);
  expect(dirtyRanks().get(91)).toBe(0);
  expect(dirtyRanks().get(92)).toBe(1);

  // Saving the first one lets the second take its place - the sticky bars
  // must never leave a gap, or sit on top of each other.
  markSuiteDirty(91, false);
  expect(dirtyRanks().has(91)).toBe(false);
  expect(dirtyRanks().get(92)).toBe(0);
});

test("marking the same suite twice does not move it", () => {
  markSuiteDirty(91, true);
  markSuiteDirty(92, true);
  markSuiteDirty(91, true);
  expect(dirtyRanks().get(91)).toBe(0);
  expect(dirtyRanks().get(92)).toBe(1);
});
```

In `src/screens/ManageCases/SuiteCases.test.tsx`, add a test for the bar itself. Follow the file's own mount helper and its drag/reorder helper if it has one; if reordering through the list is awkward in jsdom, drive `dirty` the way the existing tests do (read the file first — the "Apply order" button's enabled state is the same signal):

```tsx
test("the sticky bar appears only while the order is unsaved, and names its suite", async () => {
  mountSuite();
  await screen.findByRole("button", { name: "Apply order" });
  // Clean: the inline row is there, the sticky bar is not.
  expect(screen.queryByRole("region", { name: /unsaved order/i })).not.toBeInTheDocument();

  await reorderFirstTwo(); // the file's own helper, or a drag through CaseOrderList

  const bar = screen.getByRole("region", { name: /unsaved order/i });
  expect(within(bar).getByRole("button", { name: "Apply order" })).toBeEnabled();
  expect(within(bar).getByRole("button", { name: "Reset" })).toBeEnabled();
  expect(bar).toHaveTextContent("PBI 42 suite");

  fireEvent.click(within(bar).getByRole("button", { name: "Reset" }));
  await waitFor(() => expect(screen.queryByRole("region", { name: /unsaved order/i })).not.toBeInTheDocument());
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/screens/ManageCases/dirtySuites.test.ts src/screens/ManageCases/SuiteCases.test.tsx`
Expected: FAIL — `./dirtySuites` does not resolve, and there is no such region.

- [ ] **Step 3: Write the store**

Create `src/screens/ManageCases/dirtySuites.ts`:

```ts
import { useSyncExternalStore } from "react";

/**
 * Which suites hold an unsaved order, in the order they got one.
 *
 * Suite Management can have several suites open at once, and each renders
 * its own sticky Apply order / Reset bar. Two bars pinned to the same
 * corner would cover each other, so each one asks for its rank here and
 * offsets itself by that much. Module scope, not context: the bar needs a
 * number, not a re-render of the whole screen.
 */
const order: number[] = [];
const listeners = new Set<() => void>();
let snapshot = new Map<number, number>();

function rebuild(): void {
  snapshot = new Map(order.map((id, i) => [id, i]));
  for (const l of listeners) l();
}

export function markSuiteDirty(suiteId: number, dirty: boolean): void {
  const at = order.indexOf(suiteId);
  if (dirty) {
    if (at >= 0) return; // already listed: keep its place
    order.push(suiteId);
  } else {
    if (at < 0) return;
    order.splice(at, 1);
  }
  rebuild();
}

/** The current ranks, newest last. Stable between changes, so it is safe
 * as a `useSyncExternalStore` snapshot. */
export function dirtyRanks(): Map<number, number> {
  return snapshot;
}

function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

/** This suite's stacking rank, or null when its order is saved. */
export function useDirtyRank(suiteId: number): number | null {
  const ranks = useSyncExternalStore(subscribe, dirtyRanks, dirtyRanks);
  return ranks.get(suiteId) ?? null;
}
```

- [ ] **Step 4: Render the sticky bar**

In `src/screens/ManageCases/SuiteCases.tsx`:

Register the dirty state, and clear it on unmount so a closed suite leaves no ghost bar:

```tsx
  const rank = useDirtyRank(suiteId);
  useEffect(() => {
    markSuiteDirty(suiteId, dirty);
  }, [suiteId, dirty]);
  useEffect(() => () => markSuiteDirty(suiteId, false), [suiteId]);
```

Then, after the existing inline button row and the `CaseOrderList`, add the bar. It mirrors Run Tests' sticky control exactly (`src/screens/RunPanel/index.tsx`, the `fixed bottom-6 right-6` block), which is why the classes look the way they do:

```tsx
      {rank != null && (
        // Same sticky treatment as Run Tests' Close all: a long suite puts
        // Apply order a screen and a half above the row being dragged.
        // Stacked by rank, because several suites can be open and unsaved.
        <div
          role="region"
          aria-label={`Unsaved order in ${suiteName}`}
          className="fixed right-6 z-40 flex items-center gap-2 rounded-full border border-border bg-surface p-2.5 shadow-2xl"
          style={{ bottom: `${1.5 + rank * 3.5}rem` }}
        >
          <span className="max-w-48 truncate pl-1 text-xs text-muted">{suiteName}</span>
          <Button size="sm" disabled={busy} onClick={() => apply.mutate()}>
            <IconConfirm aria-hidden />
            {apply.isPending ? "Saving" : "Apply order"}
          </Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => cases.data && setOrder(cases.data)}>
            <IconUndo aria-hidden />
            Reset
          </Button>
        </div>
      )}
```

The inline row stays exactly as it is. `bottom` is inline because the offset is computed; everything else is a token class.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `npx vitest run src/screens/ManageCases`
Expected: PASS. If a test now finds two "Apply order" buttons and fails on ambiguity, scope its query to the inline row's container or the region — do not remove the bar to make a query pass.

- [ ] **Step 6: Typecheck and full suite**

Run: `npx tsc --noEmit`
Expected: no errors.

Run: `npx vitest run`
Expected: every file passes.

- [ ] **Step 7: Commit**

```bash
git add src/screens/ManageCases
git commit -F - <<'MSG'
feat(v2): Apply order and Reset follow you down a long suite

Reordering a suite of 150 cases left Apply order a screen and a half
above the row being dragged. Both buttons now also sit in a sticky bar
at the bottom right, only while that suite's order is unsaved, named
with the suite so it is clear what will be saved. Several suites can be
open and unsaved at once, so the bars stack instead of overlapping.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
MSG
git log -1 --stat
```

---

### Task 5: Hide New test suite when Azure DevOps says no

**Files:**
- Create: `src-tauri/src/ado/permissions.rs`
- Modify: `src-tauri/src/ado/mod.rs` (module list)
- Modify: `src-tauri/src/ado/deletion.rs` (use the shared constants/helper)
- Modify: `src-tauri/src/commands/testplan.rs` (new command) and `src-tauri/src/lib.rs` (register it)
- Create: `src-tauri/tests/permissions.rs`
- Modify: `src/screens/ManageCases/PlanTable.tsx` (the New suite button)
- Test: `src/screens/ManageCases/suites.test.tsx` or a new `src/screens/ManageCases/permissions.test.tsx`
- Regenerate: `src/bindings.ts`

**Interfaces:**
- Consumes: `plan.area_path`, already on `PlanWithSuites["plan"]` and already displayed in `PlanTable`'s header.
- Produces:
  - Rust: `AdoClient::may_manage_test_suites(&self, org: &str, project: &str, area_path: Option<&str>) -> Option<bool>` — `Some(true)`/`Some(false)` for a clear answer, `None` when the question could not be asked.
  - Command: `can_create_test_suites(organization: String, project: String, area_path: Option<String>) -> Option<bool>`, which becomes `commands.canCreateTestSuites(...)` returning `boolean | null`.
  - `permissions::{CSS_NAMESPACE_ID, MANAGE_TEST_PLANS, MANAGE_TEST_SUITES, PROJECT_NAMESPACE_ID, WORK_ITEM_DELETE}` and a shared `evaluate(...)`, which `deletion.rs` then uses instead of its own copies.

**Why this is reliable enough to gate a button on:** it is the same endpoint, namespace and permission bit the delete gate has used in the field since 1.21 (`src-tauri/src/ado/deletion.rs`, `src-tauri/tests/deletion.rs`) — `POST _apis/security/permissionevaluationbatch` with `alwaysAllowAdministrators: false`, asking about the CSS (area) namespace token for a specific area node. Creating a static suite is gated on "Manage test suites" on the plan's area, which is exactly bit 128 there. The one thing that module learned the hard way is that a root-area answer does not hold per node, so this asks about the plan's OWN `area_path`. The direction of failure is the difference: deleting hides on anything short of yes, and this one hides only on an explicit no.

- [ ] **Step 1: Write the failing Rust tests**

Create `src-tauri/tests/permissions.rs`. Read `src-tauri/tests/deletion.rs` first and reuse its mock helpers' shape (it stubs the project id, the area node and the evaluation batch):

```rust
//! The "may this user create a test suite?" question. Unlike the delete
//! gate, this one hides the button ONLY on an explicit no - see
//! src/ado/permissions.rs for why the two lean opposite ways.

use v2_lib::ado::AdoClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Project id, area node and a permission answer for the given value.
async fn with_answer(server: &MockServer, allowed: bool) {
    Mock::given(method("GET"))
        .and(path("/o/_apis/projects/p"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": "proj-guid-1" })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wit/classificationnodes/areas/Web"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "identifier": "area-guid-1" })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/o/_apis/security/permissionevaluationbatch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "evaluations": [{ "value": allowed }]
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_clear_yes_and_a_clear_no_are_both_reported() {
    let yes = MockServer::start().await;
    with_answer(&yes, true).await;
    let client = AdoClient::with_base_url("t".into(), yes.uri());
    assert_eq!(client.may_manage_test_suites("o", "p", Some("Project\\Web")).await, Some(true));

    let no = MockServer::start().await;
    with_answer(&no, false).await;
    let client = AdoClient::with_base_url("t".into(), no.uri());
    assert_eq!(client.may_manage_test_suites("o", "p", Some("Project\\Web")).await, Some(false));
}

/// The question asked is "Manage test suites on THIS area node" - the bit
/// and the namespace are pinned by reading the request body, the only way
/// a wrong-question mistake shows up (see tests/deletion.rs for the time
/// that happened).
#[tokio::test]
async fn the_permission_asked_for_is_manage_test_suites_on_the_plans_area() {
    let server = MockServer::start().await;
    with_answer(&server, true).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    client.may_manage_test_suites("o", "p", Some("Project\\Web")).await;

    let sent = server.received_requests().await.unwrap();
    let eval = sent
        .iter()
        .find(|r| r.url.path().ends_with("/permissionevaluationbatch"))
        .expect("the batch was asked");
    let body: serde_json::Value = serde_json::from_slice(&eval.body).unwrap();
    let e = &body["evaluations"][0];
    assert_eq!(e["securityNamespaceId"], "83e28ad4-2d72-4ceb-97b0-c7726d5502c3", "the CSS namespace");
    assert_eq!(e["permissions"], 128, "MANAGE_TEST_SUITES");
    assert_eq!(e["token"], "vstfs:///Classification/Node/area-guid-1", "the plan's own node");
    assert_eq!(body["alwaysAllowAdministrators"], false, "ask for the literal ACL answer");
}

/// Nothing to go on is NOT a no: the button stays and the refusal on the
/// create itself is the backstop. A failed check must not take away a
/// button the user is entitled to.
#[tokio::test]
async fn an_unanswerable_question_is_unknown_not_no() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/_apis/projects/p"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    assert_eq!(client.may_manage_test_suites("o", "p", Some("Project\\Web")).await, None);

    // A 200 whose shape says nothing is equally unknown.
    let odd = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/_apis/projects/p"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": "g" })))
        .mount(&odd)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wit/classificationnodes/areas/Web"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "identifier": "a" })))
        .mount(&odd)
        .await;
    Mock::given(method("POST"))
        .and(path("/o/_apis/security/permissionevaluationbatch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "evaluations": [] })))
        .mount(&odd)
        .await;
    let client = AdoClient::with_base_url("t".into(), odd.uri());
    assert_eq!(client.may_manage_test_suites("o", "p", Some("Project\\Web")).await, None);
}
```

Check `AdoClient::with_base_url` vs `with_base_urls` in `tests/deletion.rs` and use whichever that file uses, and copy how it turns an area path into the node URL (the `Project\Web` → `/areas/Web` mapping already exists in `deletion.rs`).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test --test permissions`
Expected: a compile error — no method `may_manage_test_suites`.

- [ ] **Step 3: Write the shared permissions module**

Create `src-tauri/src/ado/permissions.rs`. Move the constants and the request-building/answer-reading half out of `deletion.rs`, keeping their comments (they record two field failures and are the reason this is trustworthy):

```rust
//! Azure DevOps permission questions, asked through
//! `POST _apis/security/permissionevaluationbatch`.
//!
//! Two callers with opposite postures, on purpose:
//! - deleting test cases (deletion.rs) hides its button on anything short
//!   of an explicit yes: a delete that cannot work is a promise broken
//!   after the click;
//! - creating a test suite hides its button only on an explicit NO: the
//!   create itself refuses safely with a message, so an unanswerable
//!   question must not take the button away from someone entitled to it.
//!
//! Both ask with `alwaysAllowAdministrators: false` - the literal ACL
//! answer - and both ask about a SPECIFIC area node, because area
//! permissions are per node and inheritance is exactly what an org
//! overrides.
```

Then: the constants (`PROJECT_NAMESPACE_ID`, `WORK_ITEM_DELETE`, `CSS_NAMESPACE_ID`, `MANAGE_TEST_PLANS`, `MANAGE_TEST_SUITES`), the area-node → token helper, and a small `evaluate` that takes the evaluation list and returns `Result<Vec<Option<bool>>, AdoError>` (`None` per entry that is missing or not a bool). Add:

```rust
impl AdoClient {
    /// Whether this user may create or change test suites under this area.
    ///
    /// `Some(false)` is a clear no from Azure DevOps; `None` means the
    /// question could not be asked, which is NOT a no - see the module
    /// docs. `area_path` is the plan's own area, since a root-level answer
    /// does not hold per node.
    pub async fn may_manage_test_suites(
        &self,
        org: &str,
        project: &str,
        area_path: Option<&str>,
    ) -> Option<bool> {
        // ... build the single MANAGE_TEST_SUITES evaluation on the area
        // token, evaluate, and map: Ok(v) with a bool -> Some(v);
        // anything else (transport error, missing evaluation, non-bool)
        // -> None, with the raw error logged via crate::applog.
    }
}
```

Then rewrite `deletion.rs` to use the module's constants and `evaluate` rather than its own copies, leaving `can_delete_work_items`' fail-closed behaviour and every comment intact. Declare the module in `src-tauri/src/ado/mod.rs` beside the other submodules.

- [ ] **Step 4: Run both permission suites**

Run: `cd src-tauri && cargo test --test permissions --test deletion`
Expected: PASS. `deletion.rs`'s tests must pass unchanged — that is what proves the refactor kept the delete gate's behaviour.

- [ ] **Step 5: Expose the command**

In `src-tauri/src/commands/testplan.rs`, add the command beside `create_static_suite`, following that file's handler style (token via `state::get_fresh_token`, then one call into the domain module):

```rust
/// Whether the New test suite control should be offered. `None` = could not
/// ask; the frontend keeps the button, and the create itself refuses with a
/// message if Azure DevOps says no.
#[tauri::command]
#[specta::specta]
pub async fn can_create_test_suites(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    area_path: Option<String>,
) -> Result<Option<bool>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    Ok(ado::AdoClient::new(token)
        .may_manage_test_suites(&organization, &project, area_path.as_deref())
        .await)
}
```

Register it in `src-tauri/src/lib.rs` next to `testplan::create_static_suite`. Then regenerate the bindings:

Run: `cd src-tauri && cargo test --test bindings`
Expected: PASS, and `src/bindings.ts` now carries `canCreateTestSuites`. Do not hand-edit it.

- [ ] **Step 6: Write the failing frontend test**

Add to `src/screens/ManageCases/suites.test.tsx` (or a new `permissions.test.tsx` in that folder, mirroring its mount helpers):

```tsx
test("New test suite is hidden only when Azure DevOps says no", async () => {
  // A clear no: the control goes away.
  mountScreen((cmd) => (cmd === "can_create_test_suites" ? false : undefined));
  const auth = await screen.findByRole("region", { name: "Auth - Test Plan" });
  await waitFor(() =>
    expect(within(auth).queryByRole("button", { name: /New test suite/i })).not.toBeInTheDocument(),
  );
});

test("an unanswerable permission check leaves New test suite in place", async () => {
  // null = could not ask. The button stays; the create itself refuses.
  mountScreen((cmd) => (cmd === "can_create_test_suites" ? null : undefined));
  const auth = await screen.findByRole("region", { name: "Auth - Test Plan" });
  expect(within(auth).getByRole("button", { name: /New test suite/i })).toBeInTheDocument();
});
```

Check the button's real accessible name in `PlanTable.tsx` and use it exactly. `testSupport.tsx`'s mock returns `undefined` for commands it does not name — make sure that default still yields a visible button (it is the same "unknown" case).

Run: `npx vitest run src/screens/ManageCases`
Expected: FAIL — the button is there in both cases.

- [ ] **Step 7: Gate the button**

In `src/screens/ManageCases/PlanTable.tsx`, add the query and the condition:

```tsx
  // Creating a suite needs "Manage test suites" on the plan's area. Asked
  // per plan, because area permissions are per node. Hidden ONLY on a
  // clear no: a check that could not be made must not take away a button
  // the user is entitled to, and the create itself refuses with a message.
  const mayCreate = useQuery({
    queryKey: ["can-create-suites", org, project, plan.area_path],
    queryFn: () => unwrap(commands.canCreateTestSuites(org, project, plan.area_path)),
    enabled: Boolean(org && project),
    staleTime: 60 * 60_000,
    retry: false,
  });
  const hideNewSuite = mayCreate.data === false;
```

Wrap the New test suite button in `{!hideNewSuite && (…)}` and leave the dialog's own render guarded as it already is.

Note for whoever runs this: the query is memory-only on purpose. A permission answer is cheap, and caching a stale "no" across restarts would hide a button after an admin fixed the user's access.

- [ ] **Step 8: Run the tests to verify they pass**

Run: `npx vitest run src/screens/ManageCases`
Expected: PASS.

- [ ] **Step 9: Full gates**

Run: `cd src-tauri && cargo test --tests`
Expected: every suite passes, no warnings.

Run: `npx tsc --noEmit`
Expected: no errors.

Run: `npx vitest run`
Expected: every file passes.

Run: `npm run build`
Expected: succeeds.

Then `git status`: `src/bindings.ts` is expected to be modified here (the new command). Nothing else unexpected.

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src/ado src-tauri/src/commands/testplan.rs src-tauri/src/lib.rs src-tauri/tests/permissions.rs src/bindings.ts src/screens/ManageCases
git commit -F - <<'MSG'
feat(v2): New test suite is hidden when Azure DevOps refuses it

Creating a static suite needs "Manage test suites" on the plan's area,
and a user without it met the refusal only after filling in the dialog.
The control is now asked about per plan through the same permission
evaluation the delete gate uses, on the plan's OWN area node.

It hides only on an explicit no: an unanswerable check (offline, odd
response) leaves the button, and the create's own refusal stays the
backstop. The delete gate leans the other way and still does; the two
postures are documented in ado/permissions.rs, which now holds the
namespace constants and the batch call both share.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
MSG
git log -1 --stat
```

---

### Task 6: Rename Suite Lookup to Search Suites, and let a suite open in Suite Management

**Files:**
- Modify: `src/components/Sidebar.tsx:63` (the nav label)
- Modify: `src/App.tsx:148` (`TITLES.suites`), `:304` and `:373` and `:712` (comments naming the tab), plus the `Suites` render block at `:1066-1079` (pass the new prop) and the `ManageCases` render block at `:1084-1086` (pass the focus), with one new piece of state
- Modify: `src/components/CommandPalette.tsx:81` (the palette entry)
- Modify: `src/screens/Suites.tsx:100-114` (doc comment, the new prop) and its chip row at `:367-385` (the Manage chip)
- Modify: `src/screens/EditCases.tsx:25` (the handoff sentence)
- Modify: `src/screens/AiBridge.tsx:883` (the sentence naming the tab)
- Modify: `src/screens/ManageCases/index.tsx` (accept and act on `focus`)
- Modify: `README.md:92` (the section heading)
- Tests: `src/App.test.tsx` (lines ~82, ~172-184, ~816), `src/components/Sidebar.test.tsx:54`, and a new test in `src/screens/Suites.test.tsx` plus one in `src/screens/ManageCases/index.test.tsx`

**Interfaces:**
- Consumes: `plansSuitesKey` / the plan tree already in `ManageCases`; nothing from Tasks 1-5.
- Produces:
  - `Suites` gains `onManageSuite?: (planId: number, suiteId: number) => void`.
  - `ManageCases` gains `focus?: { planId: number; suiteId: number } | null` — when set, it shows that plan and opens that suite, whatever PBI is in the bar.

**Naming:** every user-visible "Suite Lookup" becomes "Search Suites". Past `src/lib/changelog.ts` entries are a record of what shipped and must NOT be edited. The internal section id stays `suites` — renaming it would break the saved last-screen preference and the tour's `where` targets for no gain.

- [ ] **Step 1: Write the failing tests**

In `src/components/Sidebar.test.tsx`, change the expectation at line 54 to `"Search Suites"`.

In `src/App.test.tsx`, change the three places that name the tab: the tab list (~line 82), the Ctrl-shortcut test's name and its heading assertion (~172-184, including the comment "Suite Lookup is finished, so it carries no pill"), and the handoff sentence (~816) to `"Showing cases handed over from Search Suites."`.

In `src/screens/Suites.test.tsx`, add a test for the new chip. Follow the file's existing mount helper and chip queries:

```tsx
test("Manage hands the suite to Suite Management", async () => {
  const managed: Array<[number, number]> = [];
  mountSuites({ onManageSuite: (planId, suiteId) => managed.push([planId, suiteId]) });
  // Open the plan's tree the way the other tests in this file do, then:
  fireEvent.click(await screen.findByRole("button", { name: "Manage" }));
  expect(managed).toEqual([[9, 91]]);
});
```

In `src/screens/ManageCases/index.test.tsx`, add a test that `focus` wins over the PBI in the bar:

```tsx
test("a suite handed over from Search Suites opens, whatever PBI is in the bar", async () => {
  // The PBI in the bar belongs to the Auth plan; the focus points at a
  // suite in the Billing plan. The focus is what the user just clicked,
  // so it wins.
  mountScreen({ pbi: { id: 42, title: "Login work", work_item_type: "Product Backlog Item" },
                focus: { planId: 10, suiteId: 101 } });
  const billing = await screen.findByRole("region", { name: "Billing - Test Plan" });
  expect(within(billing).getByRole("button", { name: /Collapse|Hide/ })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Auth - Test Plan" })).not.toBeInTheDocument();
});
```

Read `src/screens/ManageCases/testSupport.tsx` first: its mount helper takes the screen's props, so extend it to pass `focus` through, and use the plan/suite ids and names its fixtures actually define (the ids above are placeholders for whatever the Billing plan and one of its suites are called there). Assert "this suite is expanded" the way the file's own `expandSuite` helper checks it, rather than inventing a new query.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/components/Sidebar.test.tsx src/screens/Suites.test.tsx src/screens/ManageCases/index.test.tsx src/App.test.tsx`
Expected: FAIL — the label is still "Suite Lookup", there is no Manage chip, and `focus` is not a prop.

- [ ] **Step 3: Rename the tab**

Replace the user-visible string in each of these, and nowhere else:
- `src/components/Sidebar.tsx:63`: `label: "Search Suites"`
- `src/App.tsx:148`: `suites: "Search Suites",`
- `src/components/CommandPalette.tsx:81`: the item's text
- `src/screens/EditCases.tsx:25`: `Showing cases handed over from Search Suites.`
- `src/screens/AiBridge.tsx:883`: `the same view as the Search Suites tab`
- `src/screens/Suites.tsx:100`: the doc comment's first line
- `src/App.tsx:304`, `:373`, `:712`: the comments that name the tab
- `README.md:92`: the `### Suite Lookup` heading, and any sentence under it naming the tab

Leave `src/lib/changelog.ts` untouched, and leave the section id `suites` alone everywhere (Sidebar's `Section` type, the tour's `cases("suites")`, the Ctrl-shortcut order).

- [ ] **Step 4: Add the Manage chip**

In `src/screens/Suites.tsx`, add the prop:

```tsx
  /** Hand this suite to Suite Management, which opens its plan with the
   * suite unfolded. */
  onManageSuite?: (planId: number, suiteId: number) => void;
```

and, in the chip row beside View / Edit cases / Run / Report, for a suite row (not a folder row — Suite Management works on suites):

```tsx
            {onManageSuite && chip("Manage", () => onManageSuite(planId, s.id))}
```

Put it after "View" so the read actions stay together and the jump-away actions follow. `chip` already gives every one of these the same look, so nothing new is needed for styling.

- [ ] **Step 5: Carry the handoff in App**

In `src/App.tsx`, add the state beside the other handoff state (`caseSelection` is the closest precedent):

```tsx
  // A suite handed over from Search Suites' Manage chip. Cleared once
  // Suite Management has it, so going back to the tab later shows the
  // ordinary PBI-driven view rather than reopening an old click.
  const [manageFocus, setManageFocus] = useState<{ planId: number; suiteId: number } | null>(null);
```

Wire the two screens:

```tsx
                  <Suites
                    org={org}
                    project={project}
                    onOpenPbi={/* unchanged */}
                    onEditCases={/* unchanged */}
                    onManageSuite={(planId, suiteId) => {
                      setManageFocus({ planId, suiteId });
                      goToSection("manage");
                    }}
                  />
```

```tsx
                {section === "manage" && (
                  <ManageCases
                    org={org}
                    project={project}
                    pbi={pbi}
                    focus={manageFocus}
                    onFocusHandled={() => setManageFocus(null)}
                  />
                )}
```

Use `goToSection` (not bare `setSection`) so the jump behaves like every other cross-tab handoff in this file — check what `goToSection` does at `src/App.tsx:391` and follow it.

- [ ] **Step 6: Act on the focus in Suite Management**

In `src/screens/ManageCases/index.tsx`, accept the two new props and let the focus win over the PBI narrowing:

```tsx
export default function ManageCases({
  org,
  project,
  pbi,
  focus,
  onFocusHandled,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
  /** A suite the user just chose in Search Suites. It wins over the PBI in
   * the bar: it is the thing they clicked. */
  focus?: { planId: number; suiteId: number } | null;
  onFocusHandled?: () => void;
}) {
```

Then, alongside `pbiPlan`:

```tsx
  /** The plan the focused suite lives in, if it is still in the tree. */
  const focusPlan = useMemo(() => {
    if (!focus || !plans.data) return null;
    return plans.data.find(({ plan }) => plan.id === focus.planId) ?? null;
  }, [focus, plans.data]);
```

- `visible` becomes: the focused plan when there is one and `showAll` is false, else the existing PBI rule.
- The `initiallyExpanded` array passed to that plan's `PlanTable` becomes `[focus.suiteId]` (memoised on `focus.suiteId`, the way `pbiExpanded` is memoised, so `PlanTable`'s effect sees a new identity only when the suite changes).
- The notice line above the table says which suite is being shown and offers the way back, e.g. `Showing the suite you picked in Search Suites.` with a ghost button `Show this PBI's plan` (when there is a PBI) or `Show all plans`, whose click calls `onFocusHandled?.()`.
- Call `onFocusHandled?.()` too when the focused plan is not in the tree at all (a suite that has since been deleted), and fall back to the ordinary view rather than showing nothing.

Do not clear the focus merely because the screen mounted — the user must see what they clicked.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `npx vitest run src/screens/ManageCases src/screens/Suites.test.tsx src/components/Sidebar.test.tsx`
Expected: PASS.

Run: `npx vitest run src/App.test.tsx`
Expected: PASS (one re-run is allowed for that file's documented flake).

- [ ] **Step 8: Check nothing still says the old name**

Use the Grep tool for `Suite Lookup` across `src`, `src-tauri/src` and `README.md`.
Expected: matches only in `src/lib/changelog.ts` (the shipped record, deliberately untouched).

- [ ] **Step 9: Typecheck and full suite**

Run: `npx tsc --noEmit`
Expected: no errors.

Run: `npx vitest run`
Expected: every file passes, `src/ui-consistency.test.ts` included.

- [ ] **Step 10: Commit**

```bash
git add src README.md
git commit -F - <<'MSG'
feat(v2): Search Suites, with a Manage chip into Suite Management

"Suite Lookup" is now "Search Suites", which is what people call it and
what the screen does. Past changelog entries keep the old name: they are
a record of what shipped.

Each suite row also carries a Manage chip that opens Suite Management on
that suite's plan with the suite unfolded - previously the only way in
was to pick the right PBI in the bar and hope its plan was the one you
wanted. The handoff wins over the PBI narrowing while it lasts, and the
notice offers the way back.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
MSG
git log -1 --stat
```

---

## Notes for the executor

- **Tasks are independent** and can be done in any order, but Tasks 2 and 4 both edit `SuiteCases.tsx` (different regions), Tasks 3 and 5 both edit `PlanTable.tsx` (different regions), and Task 6 edits `ManageCases/index.tsx`, which no other task touches. Do them one at a time and re-read the file before editing.
- **No changelog entry and no release** are part of this plan. The changelog is written when the user asks for a release; if they do, it needs one entry naming: the findings hide button, the Suite Management speed-up, the searchable suite picker, the sticky Apply order / Reset, the New test suite button respecting permissions, and the Search Suites rename with its Manage chip.
- **If a step's anchor text does not match** the source (a reworded comment, a moved line), apply the change to the obvious equivalent place and say so in your report. If the intent is unclear, stop and ask rather than guessing.

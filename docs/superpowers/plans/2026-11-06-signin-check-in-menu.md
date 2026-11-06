# Signed-in Check Inside a Menu Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** While a person picks a recorded sign-in's signed-in check, clicks that open a menu (or any Shift-click) are carried out and not recorded, and a check picked inside a menu is saved so that replays only ask that it is on the page.

**Architecture:** The injected recorder listener (`LISTENER_JS`) lets menu-opening and Shift clicks through while picking and reports, with the picked marker, whether the pick was made inside a menu (`menu: bool`). Rust carries that flag from the marker report through `Captured` and `Draft` into a new recipe field `signed_in_in_menu`, and every place a recipe's `signed_in` marker is checked asks `Check::Present` instead of `Check::Visible` when the flag is set. A marker picked inside a menu is located by its words (a `text` locator), because a role locator is answered from the accessibility tree, which leaves out a closed (`display:none`) menu.

**Tech Stack:** Tauri 2, Rust (CDP driver, serde, specta / tauri-specta bindings), React 19 + TypeScript, vitest, the How To Use help site (`docs-site/`).

**Spec:** `docs/superpowers/specs/2026-11-06-signin-check-in-menu-design.md`

## Global Constraints

- Rust tests live only in `src-tauri/tests/suite/<module>.rs` (already declared in `suite/main.rs`); never a `#[cfg(test)]` module in `src/`, never a new file directly under `tests/`.
- `src/bindings.ts` is generated: after any `specta::Type` change run `cd src-tauri && cargo test --test bindings` and commit the result; never hand-edit it.
- UI text names no path or URL; colours only from theme tokens (`text-muted`, `text-accent`, ...); `src/ui-consistency.test.ts` is never weakened.
- A recipe saved before this change loads unchanged: the new field defaults to `false` (`#[serde(default)]`) and is not written when `false`.
- Recordings whose check was picked in plain view keep today's visible check.
- The existing refusal of a check that names the account (`check_marker_for`) stays, and applies to a check picked inside a menu too.
- The listener still never reads a field's value, still ignores untrusted events, and still stops only the picked click (with its press and release).
- Help guide: the Record sign-in entry mentions opening a menu to pick Sign out, and Shift-click; rebuild with `npm run docs:build` and commit the built site with it.
- Another task is running in the main working tree: execute in a separate git worktree on a new branch `feat/signin-check-in-menu` from `main`. One build or test command at a time. Never set `CARGO_TARGET_DIR`.
- Commits: Bash heredoc `git commit -F - <<'EOF' ... EOF`, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; confirm with `git log -1`.

## Decisions pinned here (gaps the design left to the builder)

- **Recipe field:** `SignInRecipe.signed_in_in_menu: bool`, `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`, placed right after `signed_in`. TypeScript sees `signed_in_in_menu?: boolean`.
- **How "inside a menu" is decided:** in the page, at pick time. `in menu = a click was let through while picking since the last marker` OR `the picked element is inside [role="menu"] or .dropdown-menu`. Testing visibility after the pick was rejected: the picked click is stopped, so the menu is still open and the element is visible at that moment; "visible at rest" cannot be observed without closing the menu, which the recorder must not do. The JS decision travels in the marker report (`"menu": true`) and everything after it is tested on the Rust side from the report payload, without a browser; one ignored `browser_live` test checks the JS against real Edge.
- **Menu opener:** the clicked element or up to 3 ancestors (4 elements in all: an icon or label span sits 1-2 levels inside a toggle) matches `[aria-haspopup]:not([aria-haspopup="false"]),[aria-expanded],[data-toggle="dropdown"],[data-bs-toggle="dropdown"]`, and the climb stops (not an opener) as soon as it reaches `[role="menu"],.dropdown-menu`, so an item inside an open menu is never taken for its opener.
- **Locator of an in-menu check:** its words, `Target::One(LocatorStep { text: Some(words), exact: true, .. })`, from the report's hints, when the click is not quiet and the words are 1..=80 characters (`recorder::MAX_HINT_TEXT`, made `pub`). Otherwise the existing `locate_click` result (still flagged in-menu).
- **Every check of a recipe's marker that switches** (7 sites, all to `recipe.signed_in_check()` / `home.signed_in_check()`):
  1. `src-tauri/src/autorun/signin.rs:253` (signed-in-here check used by replay-to)
  2. `src-tauri/src/autorun/signin.rs:1104` (saved session put back)
  3. `src-tauri/src/autorun/signin.rs:1183` (after the recipe's own steps)
  4. `src-tauri/src/autorun/signin.rs:1223` (after the cold-site retry)
  5. `src-tauri/src/autorun/nav.rs:863` (`load_home` with after-sign-in steps)
  6. `src-tauri/src/autorun/menu_setup.rs:186` (after menu preferences reload)
  7. `src-tauri/src/ai_bridge.rs:8903` (assistant goes home and checks it is signed in)
- **UI words:** the live and review lines read `Signed-in check: <words> (inside a menu)`. The picking hint becomes exactly `Now show us how you sign out: open the menu and click Sign out (it won't actually sign you out).` (owner's wording), followed by one line: `Hold Shift as you click to open a menu without picking it.`
- **Live event:** `RecordingEvent` gains `in_menu: bool` (`#[serde(skip_serializing_if = "std::ops::Not::not")]`), set only on a `"marker"` event. `SignInDraftView` gains `marker_in_menu: bool`.

## Review Focus

1. A menu closed with `display:none` (the common case): its items are not in the accessibility tree, so a role locator would never be found at replay - the in-menu check must be a text locator (Task 2 test `a_check_picked_inside_a_menu_is_found_by_its_words`; Task 3 live test reloads with the menu closed and asserts `Check::Present` passes and `Check::Visible` fails).
2. Recipe files written before this change, and recipes edited as JSON in Edit sign-in recipe: load with the flag off, and a recipe with the flag off is written without the field (Task 1 test `a_recipe_without_the_in_menu_flag_loads_and_is_written_without_it`).
3. Picking the person's own name from inside a menu is still refused (Task 2 test `a_check_inside_a_menu_that_names_the_account_is_still_refused`).
4. Pick again after an in-menu pick, choosing something in plain view: the flag follows the last pick, not any earlier one (Task 2 test `the_last_pick_decides_whether_the_check_is_inside_a_menu`; Task 3 string assertion that the page resets its flag after each marker).
5. An in-menu pick with no words (a quiet or icon-only item): falls back to the accessibility-tree locator and stays flagged, rather than becoming unreadable (Task 2 test `a_wordless_check_inside_a_menu_falls_back_to_the_tree_and_keeps_the_flag`).

Known limit, not tested: a Shift-click on a real link opens a new browser window (browser default). The hint offers Shift only for menus the app does not recognise; it is not prevented, because stopping the default would also hide the click from page frameworks that ignore prevented events.

---

## File Structure

| File | Responsibility in this change |
|---|---|
| `src-tauri/src/autorun/recipe.rs` | `signed_in_in_menu` field; `signed_in_check(in_menu)` and `SignInRecipe::signed_in_check()` |
| `src-tauri/src/autorun/nav.rs` | `Home.signed_in_in_menu`, `Home::signed_in_check()`, site 5 |
| `src-tauri/src/autorun/signin.rs` | sites 1-4 |
| `src-tauri/src/autorun/menu_setup.rs` | site 6 |
| `src-tauri/src/ai_bridge.rs` | site 7 |
| `src-tauri/src/autorun/recorder.rs` | `MAX_HINT_TEXT` made `pub` |
| `src-tauri/src/autorun/signin_recorder.rs` | listener JS; `SignInClick.menu`; `menu_marker_from_hints`, `locate_marker`; `Captured`/`Draft`/`Seen`/`finish`/`SignInDraftView` carry the flag; `Draft::recipe` writes it |
| `src-tauri/src/commands/autorun_record_signin.rs` | `listen` sets `in_menu` on the marker event; `auto_run_record_sign_in_stop` builds the `Draft` with the flag |
| `src-tauri/src/events.rs`, `src-tauri/src/commands/autorun_record.rs` | `RecordingEvent.in_menu`; module recorder literals set `false` |
| `src/screens/AutoRun/RecordSignInDialog.tsx` | "(inside a menu)" on both lines; Shift hint line |
| `docs-site/src/content/auto-run.ts`, `src-tauri/help/` | Record sign-in entry text, rebuilt site |
| Tests | `tests/suite/autorun_recipe.rs`, `autorun_signin.rs`, `autorun_nav.rs`, `common.rs`, `autorun_record_signin.rs`, `browser_live.rs`; `src/screens/AutoRun/RecordSignInDialog.test.tsx` |

---

### Task 1: The recipe flag, and replays that ask "present" for it

**Files:**
- Modify: `src-tauri/src/autorun/recipe.rs:288-342` (struct), add the two functions after the struct's `impl` block
- Modify: `src-tauri/src/autorun/nav.rs:699-723` (`Home`), `:863`
- Modify: `src-tauri/src/autorun/signin.rs:253, 1104, 1183, 1223`
- Modify: `src-tauri/src/autorun/menu_setup.rs:186`
- Modify: `src-tauri/src/ai_bridge.rs:8900-8908`
- Modify: `src-tauri/src/autorun/signin_recorder.rs:702-714` (literal: `signed_in_in_menu: false` for now; Task 2 wires it)
- Modify: `src/bindings.ts` (regenerated)
- Test: `src-tauri/tests/suite/autorun_recipe.rs`, `autorun_signin.rs`, `autorun_nav.rs`, `common.rs`, `autorun_record_signin.rs:432` (add `signed_in_in_menu: false` to the expected literal)

**Interfaces:**
- Produces:
  - `pub signed_in_in_menu: bool` on `SignInRecipe` (doc: "The signed-in check was picked inside a menu that is closed at rest: a sign-in asks that it is on the page, not that it is visible. Off for every recipe saved before this existed."). Change `signed_in`'s doc to "Exactly one visible match of this means "signed in" - or, with `signed_in_in_menu`, at least one match on the page."
  - `pub fn signed_in_check(in_menu: bool) -> crate::browser::expect::Check<'static>` in `recipe.rs` (`Present` when `in_menu`, else `Visible`)
  - `impl SignInRecipe { pub fn signed_in_check(&self) -> Check<'static> }`
  - `pub signed_in_in_menu: bool` on `nav::Home` (set by `Home::of`), and `impl Home { pub fn signed_in_check(&self) -> Check<'static> }`
  - `pub fn stateful_app_marker_hidden() -> (ScriptedDriver, StatefulApp)` in `tests/suite/common.rs`

- [ ] **Step 1: Write the failing tests**

In `autorun_recipe.rs`:

```rust
#[test]
fn a_recipe_without_the_in_menu_flag_loads_and_is_written_without_it() {
    let r = recipe(sample());
    assert!(!r.signed_in_in_menu);
    assert!(serde_json::to_value(&r).unwrap().get("signed_in_in_menu").is_none());
    assert!(matches!(r.signed_in_check(), Check::Visible));
    let mut v = sample();
    v["signed_in_in_menu"] = json!(true);
    let r = recipe(v);
    assert!(r.signed_in_in_menu);
    assert_eq!(serde_json::to_value(&r).unwrap()["signed_in_in_menu"], json!(true));
    assert!(matches!(r.signed_in_check(), Check::Present));
}
```

In `autorun_nav.rs`:

```rust
#[test]
fn home_carries_the_recipes_in_menu_check() {
    let mut r = common::menu_recipe();
    assert!(matches!(Home::of(&r).signed_in_check(), Check::Visible));
    r.signed_in_in_menu = true;
    assert!(Home::of(&r).signed_in_in_menu);
    assert!(matches!(Home::of(&r).signed_in_check(), Check::Present));
}
```

In `common.rs`, `stateful_app_marker_hidden()`: the same page as `stateful_app(false, None)`, except that `#marker`, once signed in, is found only by a lookup that includes hidden elements (the CSS lookup's second argument, `visibleOnly`, is `false`). Do it by moving `stateful_app`'s body into a private `stateful_app_with(cookie_is_good, broken_selector, marker_hidden: bool)`, recording `params["arguments"][1]["value"].as_bool().unwrap_or(true)` beside `last_selector`, and answering `#marker` as absent when `marker_hidden` and that flag is `true`. `stateful_app` calls it with `false`.

In `autorun_signin.rs`:

```rust
#[tokio::test]
async fn a_check_inside_a_menu_signs_in_when_it_is_on_the_page_but_not_visible() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, _state) = common::stateful_app_marker_hidden();
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(!out.ok, "a visible check still needs it visible: {}", out.detail);

    let dir = tempfile::tempdir().unwrap();
    let (mut d, _state) = common::stateful_app_marker_hidden();
    let in_menu = SignInRecipe { signed_in_in_menu: true, ..recipe() };
    let out = sign_in(&mut d, dir.path(), &in_menu, &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cd src-tauri && cargo test --test suite autorun_recipe::a_recipe_without_the_in_menu_flag`
Expected: compile error, no field `signed_in_in_menu` / no method `signed_in_check`.

- [ ] **Step 3: Implement**

Add the field and the functions listed under Interfaces. Replace `Check::Visible` with `recipe.signed_in_check()` at the four `signin.rs` sites and with `home.signed_in_check()` at `nav.rs:863`, `menu_setup.rs:186` and `ai_bridge.rs:8903` (there, `crate::browser::expect::Check::Visible` becomes `home.signed_in_check()`). Add `signed_in_in_menu: false` to the literal in `signin_recorder.rs:702` and to the expected literal in `autorun_record_signin.rs:432`.

- [ ] **Step 4: Prove no marker check was missed**

Run: `grep -rn "signed_in, Check::Visible\|signed_in,$" src-tauri/src`
Expected: no line that checks a recipe's or home's `signed_in` with `Visible` remains (the `ai_bridge.rs` call is multi-line; read 8900-8908 to confirm).

- [ ] **Step 5: Run the tests to verify they pass**

Run, one at a time: `cd src-tauri && cargo test --test suite autorun_recipe::`, then `autorun_signin::`, then `autorun_nav::`, then `autorun_record_signin::`, then `autorun_menu_setup::`
Expected: all PASS.

- [ ] **Step 6: Regenerate bindings and typecheck**

Run: `cd src-tauri && cargo test --test bindings`, then `npx tsc --noEmit`
Expected: `src/bindings.ts` gains `signed_in_in_menu?: boolean` on both `SignInRecipe_Serialize` and `SignInRecipe_Deserialize`; tsc clean.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/autorun/recipe.rs src-tauri/src/autorun/nav.rs src-tauri/src/autorun/signin.rs src-tauri/src/autorun/menu_setup.rs src-tauri/src/ai_bridge.rs src-tauri/src/autorun/signin_recorder.rs src-tauri/tests/suite/autorun_recipe.rs src-tauri/tests/suite/autorun_signin.rs src-tauri/tests/suite/autorun_nav.rs src-tauri/tests/suite/common.rs src-tauri/tests/suite/autorun_record_signin.rs src/bindings.ts
git commit -F - <<'EOF'
feat(v2): a sign-in check kept inside a menu is checked as present

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 2: The recorder carries "inside a menu" from the pick to the recipe

**Files:**
- Modify: `src-tauri/src/autorun/recorder.rs:132` (`pub const MAX_HINT_TEXT`)
- Modify: `src-tauri/src/autorun/signin_recorder.rs:234-243` (`SignInClick`), `:270-285` (`Captured`, `Seen`), `:410-434` (new functions beside `locate_click`), `:515-590` (`capture`, `finish`), `:620-717` (`SignInDraftView`, `Draft`, `view`, `recipe`)
- Modify: `src-tauri/src/events.rs:313-328` (`RecordingEvent.in_menu`), `src-tauri/src/commands/autorun_record.rs:326-340` (three literals: `in_menu: false`)
- Modify: `src-tauri/src/commands/autorun_record_signin.rs:128-157, 269-271`
- Modify: `src/bindings.ts` (regenerated); `src/screens/AutoRun/RecordSignInDialog.test.tsx:29-38` (`DRAFT` gains `marker_in_menu: false`)
- Test: `src-tauri/tests/suite/autorun_record_signin.rs`; existing literals there at `:364, :396-408, :548-557, :666, :714, :734, :831-840` gain `marker_in_menu: false`; `:272` becomes `Seen::Marker(t, _)`; `:300` destructures three values. `src-tauri/tests/suite/browser_live.rs:1654` `Draft` literal gains `marker_in_menu: third.marker_in_menu`.

**Interfaces:**
- Consumes: `SignInRecipe.signed_in_in_menu` (Task 1).
- Produces:
  - `SignInClick { #[serde(default)] pub menu: bool, .. }` - the page's `"menu"` on a marker report
  - `pub fn menu_marker_from_hints(reported: &SignInClick) -> Option<Target>`
  - `pub async fn locate_marker<D: Driver>(d: &mut D, reported: &SignInClick) -> Result<Target, ()>` - `menu_marker_from_hints` when `reported.menu` and it gives one, else `locate_click`
  - `Captured { pub marker_in_menu: bool, .. }` (doc: "The picked check was inside a menu. Follows the last pick.")
  - `Seen::Marker(&'a Target, bool)`
  - `pub fn finish(captured: Captured) -> Result<(Vec<Step>, Option<Target>, bool), String>`
  - `Draft { pub marker_in_menu: bool, .. }`; `SignInDraftView { pub marker_in_menu: bool, .. }` (true only with a marker)
  - `Draft::recipe` sets `signed_in_in_menu: self.marker_in_menu`
  - `RecordingEvent { #[serde(skip_serializing_if = "std::ops::Not::not")] pub in_menu: bool, .. }` (doc: "A "marker" picked inside a menu."); `listen` sets it from `Seen::Marker`'s flag

- [ ] **Step 1: Write the failing tests** (in `autorun_record_signin.rs`; a helper `menu_report(i: u32, text: &str) -> Event` is `click_report("marker", i, text)` with `"menu": true` added to the payload)

```rust
#[test]
fn a_check_picked_inside_a_menu_is_found_by_its_words() {
    let c: SignInClick = serde_json::from_value(json!({ "doc": "d1", "i": 0, "menu": true, "tag": "a", "role": "menuitem", "label": "", "text": " Sign \n out " })).unwrap();
    assert!(c.menu);
    assert_eq!(menu_marker_from_hints(&c), Some(Target::One(LocatorStep { text: Some("Sign out".into()), exact: true, ..LocatorStep::default() })));
    let quiet = SignInClick { quiet: true, ..c.clone() };
    assert_eq!(menu_marker_from_hints(&quiet), None);
    let long = serde_json::from_value::<SignInClick>(json!({ "doc": "d1", "i": 0, "menu": true, "text": "x".repeat(81) })).unwrap();
    assert_eq!(menu_marker_from_hints(&long), None);
    let old: SignInClick = serde_json::from_value(json!({ "doc": "d1", "i": 0, "text": "Sign out" })).unwrap();
    assert!(!old.menu, "a report without the flag is a plain pick");
}

#[tokio::test]
async fn a_check_inside_a_menu_reaches_the_draft_the_view_the_event_and_the_recipe() {
    // held element 0 is in the tree as button "Sign out": the in-menu pick
    // must still be named by its words, not by the tree.
    let mut d = page_with(vec![Some(tree(0, "button", "Sign out"))], no_evals());
    d.events.push_back(field_report(1, false, "user")); // a step, so finish succeeds
    d.events.push_back(menu_report(0, "Sign out"));
    let mut heard: Vec<RecordingEvent> = vec![];
    let captured = listen(&mut d, &AtomicBool::new(true), &AtomicBool::new(false), &AtomicBool::new(true), &mut |e| heard.push(e)).await;
    let words = Target::One(LocatorStep { text: Some("Sign out".into()), exact: true, ..LocatorStep::default() });
    assert_eq!(captured.marker, Some(words.clone()));
    assert!(captured.marker_in_menu);
    let marker_event = heard.iter().find(|e| e.kind == "marker").unwrap();
    assert!(marker_event.in_menu && marker_event.readable == "text \"Sign out\"");
    assert_eq!(serde_json::to_value(&heard[0]).unwrap().get("in_menu"), None, "only a marker carries it");
    let (steps, marker, in_menu) = finish(captured).unwrap();
    let draft = Draft { organization: "acme".into(), project: "Web".into(), start_url: "https://hr.example.internal/".into(), steps, marker, marker_in_menu: in_menu };
    assert!(draft.view().marker_in_menu);
    let recipe = draft.recipe(&[choice(FieldRole::Username, "")], None).unwrap();
    assert!(recipe.signed_in_in_menu);
    assert_eq!(recipe.signed_in, words);
}

#[tokio::test]
async fn the_last_pick_decides_whether_the_check_is_inside_a_menu() {
    let mut d = page_with(vec![None, None], no_evals());
    d.events.push_back(menu_report(0, "Sign out"));
    d.events.push_back(click_report("marker", 1, "Home"));
    let captured = capture(&mut d, &AtomicBool::new(true), &AtomicBool::new(false), &AtomicBool::new(true), &mut |_| {}).await;
    assert_eq!(captured.marker.as_ref().map(Target::describe), Some("text \"Home\"".to_string()));
    assert!(!captured.marker_in_menu);
}

#[tokio::test]
async fn a_wordless_check_inside_a_menu_falls_back_to_the_tree_and_keeps_the_flag() {
    let mut d = page_with(vec![Some(tree(0, "menuitem", "Sign out"))], no_evals());
    d.events.push_back(menu_report(0, ""));
    let captured = capture(&mut d, &AtomicBool::new(true), &AtomicBool::new(false), &AtomicBool::new(true), &mut |_| {}).await;
    assert_eq!(captured.marker, Some(exact_role("menuitem", "Sign out")));
    assert!(captured.marker_in_menu);
}

#[test]
fn a_check_inside_a_menu_that_names_the_account_is_still_refused() {
    let c: SignInClick = serde_json::from_value(json!({ "doc": "d1", "i": 0, "menu": true, "text": "kim" })).unwrap();
    let marker = menu_marker_from_hints(&c).unwrap();
    assert_eq!(check_marker_for(&marker, &account()).unwrap_err(), MARKER_NAMES_ACCOUNT);
}
```

(`account()` is `common::account`, whose username is checked here; if it is not `kim`, use its username in the `text`.) Also extend `a_draft_becomes_a_recipe_with_placeholders_fixed_text_and_the_default_settings` with: `draft().view().marker_in_menu == false`, and `Draft { marker: None, marker_in_menu: true, ..draft() }.view().marker_in_menu == false`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cd src-tauri && cargo test --test suite autorun_record_signin::`
Expected: compile errors (no `menu`, `menu_marker_from_hints`, `marker_in_menu`, `in_menu`).

- [ ] **Step 3: Implement** the Interfaces above. In `capture`, a `Report::Marker` that `locate_marker` names sets both `marker` and `marker_in_menu` (from `click.menu`); one it cannot name changes neither. `auto_run_record_sign_in_stop` builds `Draft` with the third value from `finish`; its log line is unchanged. Update every existing test literal listed under Files.

- [ ] **Step 4: Run the tests to verify they pass**

Run, one at a time: `cd src-tauri && cargo test --test suite autorun_record_signin::`, then `cargo test --test suite autorun_record::`
Expected: PASS. Then `cargo test --test suite browser_live:: --no-run` - Expected: compiles.

- [ ] **Step 5: Regenerate bindings, fix the test fixture, typecheck, run the dialog tests**

Run: `cd src-tauri && cargo test --test bindings`. Add `marker_in_menu: false` to `DRAFT` in `RecordSignInDialog.test.tsx`. If the generated `RecordingEvent` shows `in_menu: boolean` (required) rather than `in_menu?: boolean`, add `in_menu: false` to that file's `step()` helper and its two inline `RecordingEvent` literals. Then `npx tsc --noEmit`, then `npx vitest run src/screens/AutoRun/RecordSignInDialog.test.tsx`
Expected: tsc clean; vitest PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/autorun/recorder.rs src-tauri/src/autorun/signin_recorder.rs src-tauri/src/events.rs src-tauri/src/commands/autorun_record.rs src-tauri/src/commands/autorun_record_signin.rs src-tauri/tests/suite/autorun_record_signin.rs src-tauri/tests/suite/browser_live.rs src/bindings.ts src/screens/AutoRun/RecordSignInDialog.test.tsx
git commit -F - <<'EOF'
feat(v2): a sign-in check picked inside a menu is saved as such

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 3: The listener lets menu and Shift clicks through while picking

**Files:**
- Modify: `src-tauri/src/autorun/signin_recorder.rs:79-96` (`clicked` takes the menu flag), `:153-178` (pick-mode handling), module doc `:5-11` (mention the let-through clicks)
- Test: `src-tauri/tests/suite/autorun_record_signin.rs:138-172` (update the pinned strings), `src-tauri/tests/suite/browser_live.rs` (new ignored test)

**Interfaces:**
- Consumes: the `"menu"` payload field read by `SignInClick` (Task 2).
- Produces: marker reports carrying `menu: <bool>`; no Rust API change.

The algorithm (the tests pin its strings; the body is given because they do not determine it):

```js
  // While picking, a click that opens a menu - or any click with Shift
  // held - is carried out, and is neither recorded nor picked: the person
  // can open the menu that holds Sign out.
  const OPENER = '[aria-haspopup]:not([aria-haspopup="false"]),[aria-expanded],[data-toggle="dropdown"],[data-bs-toggle="dropdown"]';
  const IN_MENU = '[role="menu"],.dropdown-menu';
  const opensMenu = (el) => {
    for (let n = el, up = 0; n && up <= 3; n = n.parentElement, up++) {
      if (n.matches(IN_MENU)) return false;
      if (n.matches(OPENER)) return true;
    }
    return false;
  };
  const letThrough = (e) => {
    const el = elementOf(e.target);
    return e.shiftKey || (!!el && opensMenu(el));
  };
  // A click was let through since the last pick: what is picked next was
  // reached through a menu.
  let opened = false;
```

`pressWhilePicking` becomes `if (e.isTrusted && window.__tcmRecPick && !letThrough(e)) hush(e);`. In the click listener's pick branch, before `window.__tcmRecPick = false;`: `if (letThrough(e)) { opened = true; return; }`; the marker becomes `clicked('marker', el, opened || !!el.closest(IN_MENU));` followed by `opened = false;`. `clicked(ev, el, menu)` adds `menu: !!menu,` to the payload (an ordinary click passes nothing, so `false`).

- [ ] **Step 1: Update the listener test to the new behaviour (fails)**

In `the_listener_never_reads_what_was_typed_and_stops_only_the_picked_click`, replace the assertion on line 156 and keep the counts:

```rust
assert!(LISTENER_JS.contains("if (e.isTrusted && window.__tcmRecPick && !letThrough(e)) hush(e);"));
assert!(LISTENER_JS.contains("if (letThrough(e)) { opened = true; return; }"), "a menu or Shift click is let through");
assert!(LISTENER_JS.contains("e.shiftKey ||"));
assert!(LISTENER_JS.contains("up <= 3"), "the opener is looked for up to 3 levels up");
for attr in ["[aria-haspopup]:not([aria-haspopup=\"false\"])", "[aria-expanded]", "[data-toggle=\"dropdown\"]", "[data-bs-toggle=\"dropdown\"]"] {
    assert!(LISTENER_JS.contains(attr), "{attr} opens a menu");
}
assert!(LISTENER_JS.contains("if (n.matches(IN_MENU)) return false;"), "an item inside an open menu is not its opener");
assert!(LISTENER_JS.contains("clicked('marker', el, opened || !!el.closest(IN_MENU));\n      opened = false;"));
assert!(LISTENER_JS.contains("menu: !!menu,"));
```

Keep `isTrusted` count 5, `preventDefault` 1, `stopImmediatePropagation` 1, `hush(e)` 2. Change the "only the picked click is stopped" distance check to measure from `"window.__tcmRecPick = false;\n      hush(e);"` (it must exist once).

- [ ] **Step 2: Run it to verify it fails**

Run: `cd src-tauri && cargo test --test suite autorun_record_signin::the_listener_never_reads`
Expected: FAIL on the `letThrough` assertion.

- [ ] **Step 3: Implement** the JS above in `LISTENER_JS`.

- [ ] **Step 4: Run the module to verify it passes**

Run: `cd src-tauri && cargo test --test suite autorun_record_signin::`
Expected: PASS.

- [ ] **Step 5: Write the live test** (ignored, like its neighbours) in `browser_live.rs`, `a_menu_opened_while_picking_is_carried_out_and_its_check_is_found_closed`. It writes this page to a temp file and opens it as `file:///...` the way `upload_reaches_a_file_input_directly_and_through_the_chooser` does, after `signin_recorder::prepare(&mut live.cdp, &url, &timing())`:

```html
<button id="me" aria-haspopup="menu" onclick="document.getElementById('m').style.display='block'">Kim Lee</button>
<ul id="m" role="menu" style="display:none"><li><a href="#" role="menuitem" onclick="document.body.dataset.out='1'; return false">Sign out</a></li></ul>
<div id="plain" onclick="document.getElementById('h').style.display='block'">Account</div>
<p id="h" style="display:none">Help</p>
```

Assertions: with `PICK_ON_JS` evaluated, a real click on `#me` opens `#m` (read `getComputedStyle(m).display === 'block'` from the page); a real mouse press and release on `#plain`'s centre with `modifiers: 8` (Shift) through `Input.dispatchMouseEvent` shows `#h`; a real click on `#m a` does not set `document.body.dataset.out`; `capture` with `pick = true` returns no steps, `marker` describing `text "Sign out"`, `marker_in_menu == true`. Then navigate to the page again (menu closed) and assert `expect(&mut live.cdp, &marker, Check::Present, 2000, 50).await.ok` and `!expect(.., Check::Visible, 500, 50).await.ok`.

- [ ] **Step 6: Run the live test on purpose**

Run: `cd src-tauri && cargo test --test suite browser_live::a_menu_opened_while_picking -- --ignored --test-threads=1`
Expected: PASS (needs Edge; if the machine cannot start it, record that it was not run - never mark it passed).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/autorun/signin_recorder.rs src-tauri/tests/suite/autorun_record_signin.rs src-tauri/tests/suite/browser_live.rs
git commit -F - <<'EOF'
feat(v2): picking a sign-in check lets menu and Shift clicks through

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 4: The dialog shows "(inside a menu)" and the Shift hint

**Files:**
- Modify: `src/screens/AutoRun/RecordSignInDialog.tsx:45` (recording phase gains `markerInMenu: boolean`), `:149`, `:214`, `:419-430`, `:563-566`
- Test: `src/screens/AutoRun/RecordSignInDialog.test.tsx`

**Interfaces:**
- Consumes: `RecordingEvent.in_menu` and `SignInDraftView.marker_in_menu` (Task 2 bindings).

- [ ] **Step 1: Write the failing tests**

```tsx
test("a check picked inside a menu says so, live and in the review, and the hint offers Shift", async () => {
  mount(recorder(undefined, (cmd) => (cmd === "auto_run_record_sign_in_stop" ? { ...DRAFT, marker: 'text "Sign out"', marker_in_menu: true } : undefined)));
  await startRecording();
  fireEvent.click(screen.getByRole("button", { name: "I'm signed in" }));
  expect(await screen.findByText("Hold Shift as you click to open a menu without picking it.")).toBeInTheDocument();
  await send({ ...step("marker", 0, 'text "Sign out"'), in_menu: true });
  expect(screen.getByText(/text "Sign out" \(inside a menu\)/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Finish" }));
  await screen.findByRole("button", { name: "Check and save" });
  expect(screen.getByText(/text "Sign out" \(inside a menu\)/)).toBeInTheDocument();
});

test("a check picked in plain view carries no menu note", async () => {
  mount(recorder());
  await toReview();
  expect(screen.queryByText(/inside a menu/)).not.toBeInTheDocument();
});
```

- [ ] **Step 2: Run them to verify they fail**

Run: `npx vitest run src/screens/AutoRun/RecordSignInDialog.test.tsx`
Expected: FAIL - no Shift line, no "(inside a menu)".

- [ ] **Step 3: Implement.** The marker event sets `marker: p.readable, markerInMenu: !!p.in_menu`; a fresh recording starts with `markerInMenu: false`. Both "Signed-in check:" lines append ` (inside a menu)` when their flag is set (the review: only when `draft.marker` is non-empty). The picking hint (today: "Now click Sign out, or something every signed-in account sees - not your own name. That click is not carried out.") is replaced by exactly `Now show us how you sign out: open the menu and click Sign out (it won't actually sign you out).` (owner's wording, 2026-11-06), and under it a second `<p className="text-xs text-accent">` reads exactly `Hold Shift as you click to open a menu without picking it.` Update any existing test that pins the old hint text, and add the new sentence to the Task 4 test's assertions.

- [ ] **Step 4: Run tests, typecheck, and the UI gate**

Run, one at a time: `npx vitest run src/screens/AutoRun/RecordSignInDialog.test.tsx`, `npx vitest run src/ui-consistency.test.ts`, `npx tsc --noEmit`
Expected: all PASS / clean.

- [ ] **Step 5: Commit**

```bash
git add src/screens/AutoRun/RecordSignInDialog.tsx src/screens/AutoRun/RecordSignInDialog.test.tsx
git commit -F - <<'EOF'
feat(v2): Record sign-in marks a check picked inside a menu

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 5: The help guide

**Files:**
- Modify: `docs-site/src/content/auto-run.ts:760-768` (`record-sign-in` entry's `does`)
- Modify: `src-tauri/help/` (built)
- Modify: `docs/backlog.html` (add the item as built, if the owner's backlog lists open Auto Run items)

- [ ] **Step 1: Extend the entry.** Append to `does`: ` While you pick the signed-in check, a click that opens a menu is carried out and not recorded, so you can open the menu under your name and pick **Sign out**; hold **Shift** as you click to open a menu the app does not recognise. A check picked inside a menu is marked **(inside a menu)**, and a sign-in then only asks that it is on the page.` No screenshot changes: the Setup row this entry points at is unchanged.

- [ ] **Step 2: Build the site**

Run: `npm run docs:build`
Expected: build succeeds; `src-tauri/help/` changes.

- [ ] **Step 3: Run the guide's tests**

Run: `npx vitest run docs-site/src`
Expected: PASS (`built.test.ts` proves the built page is not stale; `guard.test.ts` that no hidden feature is named).

- [ ] **Step 4: Commit**

```bash
git add docs-site/src/content/auto-run.ts src-tauri/help docs/backlog.html
git commit -F - <<'EOF'
docs(v2): the help guide explains picking a sign-in check inside a menu

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

## Final gate (after Task 5, one command at a time)

- `cd src-tauri && cargo test --tests` - Expected: PASS.
- `npm test` - Expected: PASS (`src/App.test.tsx` has a known load flake: one failure that passes on re-run is that).
- `npx tsc --noEmit` - Expected: clean.
- Owed by hand (a person, live site): record a sign-in on a site whose Sign out sits under the user's name; open the menu while picking, pick Sign out, see "(inside a menu)", Check and save, then replay a case with another account.

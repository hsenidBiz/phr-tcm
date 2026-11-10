# Agent-built Sign-in Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the assistant inspect a sign-in page (or an unknown change-password page), propose a tested sign-in recipe for the active environment, and have the person approve or reject it in the app; discovery and every live-app command recover through that instead of stopping.

**Architecture:** Three new focused Rust modules: `autorun/signin_inspect.rs` reads a page's controls (never values) and counts visible password fields, `autorun/signin_attempts.rs` caps sign-in attempts per account, and `autorun/signin_proposal.rs` stores, merges and describes the one pending proposal per environment and project. `commands/autorun_signin_proposal.rs` holds the bridge handlers (generic over `Browsers`, so tests drive them with fakes) and the Tauri commands (view, approve, reject); `ai_bridge.rs` and `mcp.rs` only route. The UI adds one component, `SignInProposalNotice.tsx`, under the Sign-in row of the Setup panel.

**Tech Stack:** Rust (Tauri 2, tauri-specta, tokio, serde), React 19 + TypeScript, @tanstack/react-query, vitest + Testing Library, the docs-site builder.

**Spec:** `docs/superpowers/specs/2026-11-10-agent-built-sign-in-design.md` (binding; its section "Calls made after the builder check" settles anything the earlier sections leave open).

## Global Constraints

- Rust tests only as modules in `src-tauri/tests/suite/`, each with a `mod` line in `src-tauri/tests/suite/main.rs`; never `#[cfg(test)]` in `src/`, never a new file directly under `tests/`.
- Run one module: `cd src-tauri && cargo test --test suite <module>::`. Frontend: `npx vitest run <path>`. Typecheck: `npx tsc --noEmit`.
- A test touching process-wide state (a static, the log tail, account leases) takes its lock from `suite/serial.rs` (`serial::log_tail()`, `serial::account_leases()`, `serial::autorun()`); the new attempt counter gets its own lock `serial::sign_in_attempts()`.
- `src/bindings.ts` is regenerated with `cd src-tauri && cargo test --test bindings` after any `specta::Type` / command change; never hand-edit it. The `tests/bindings.rs` token gate stays green: no doc comment or field on a specta type contains `cookie`, `bearer` or `*_token`.
- No HTTP DELETE anywhere (not touched here).
- User-facing text names no URL, host or file path: page paths via `browser::save_guard::path_of` only; transport errors logged, never shown.
- No password or typed value reaches the assistant, the webview, a log line or an answer. The inspect JavaScript never reads an input's `.value`.
- Theme tokens only (`text-text`, `text-muted`, `bg-surface`, `border-border`, `text-success`, `text-danger`...); icons from `src/lib/actionIcons.ts`; `src/ui-consistency.test.ts` is never weakened.
- Attempt cap: at most **3** sign-in attempts per account per sign-in problem (`MOST_SIGN_IN_ATTEMPTS = 3`); inspect-with-account, trials and the follow-up `check_autorun_sign_in` all count; inspect without an account never signs in.
- Proposal limits for the agent: at most **2** proposals per sign-in problem per session; rejected proposal, second failed trial, wrong password, locked account, captcha or one-time code means report and stop.
- Resets only on a test environment with Reset expired passwords on. A trial never sends the change-password form. The "Also turn on Reset expired passwords" tick box is shown only on test environments, unticked by default.
- A proposal never replaces an environment's own address (`for_saving`); the draft supplies only `start_url`, `steps`, `signed_in`, `signed_in_in_menu`, `password_reset`; everything else comes from the saved recipe.
- Unknown change-password detection: two or more visible password fields, with no username condition.
- No em or en dashes in text an assistant reads (guide, tool descriptions, command bodies).
- Commits via Bash heredoc only: `git commit -F - <<'EOF'` ... `EOF`, message ending with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; confirm with `git log -1`.
- Never stage `.test-cases/`; stage files by name, never `git add -A`.
- One build or test command at a time; never two suites at once (the machine is shared).
- Never set `CARGO_TARGET_DIR` or `--target-dir`; the one cache is `src-tauri/target`.
- No changelog entry in this plan (the release bump adds it).

## Review Focus

1. **A trial resets a password.** `sign_in_core` resets on the reset page when the switch is on; a trial with a `password_reset` part on a test environment must not. Test `a_trial_on_the_reset_page_never_sends_the_form` (Task 2).
2. **A typed value leaks through inspect.** The page's DOM holds a typed username/password; the inspect answer must not. Tests `inspect_js_never_reads_a_value` and `inspect_answer_carries_no_value_even_when_the_page_sends_one` (Task 4).
3. **Approve overwrites a recipe changed meanwhile, or replaces the environment's address.** Tests `approve_refuses_when_the_saved_recipe_changed` and `approve_keeps_the_environments_own_address` (Task 6).
4. **The attempt cap is bypassed through `check_autorun_sign_in`, or blocks ordinary checks.** Tests `the_follow_up_check_counts_and_the_fourth_attempt_is_refused` and `a_check_with_no_open_problem_is_not_counted` (Task 7).
5. **The unknown change-password page misfires on an ordinary login page or hides a wrong password.** Tests `one_password_field_is_not_a_change_password_page` and `an_unknown_change_password_page_is_password_expired` (Task 3).

---

## File Structure

| File | Create/Modify | Responsibility |
|---|---|---|
| `src-tauri/src/autorun/recipe.rs` | Modify | `PasswordResetPage::no_current_password`; validate it |
| `src-tauri/src/autorun/signin.rs` | Modify | `fill_reset_form` skips the current field; `Attempt` mode (trial never resets, never saves a session); `sign_in_trial`; unknown change-password sentence; `is_password_expired` |
| `src-tauri/src/autorun/signin_inspect.rs` | Create | `INSPECT_JS`, `PASSWORD_FIELDS_JS`, `SignInPageView`, `read_sign_in_page`, `visible_password_fields`, `reset_fields_showing` |
| `src-tauri/src/autorun/signin_attempts.rs` | Create | Per data folder + account attempt counter (cap 3) |
| `src-tauri/src/autorun/signin_proposal.rs` | Create | Proposal store (`shared/proposals/signin-<env>-<slug>.json`), recipe hash, merge rule, summary words, webview view |
| `src-tauri/src/autorun/mod.rs` | Modify | `pub mod` lines for the three modules |
| `src-tauri/src/commands/autorun_signin_proposal.rs` | Create | `inspect_in`, `propose_in` (bridge handlers over `Browsers`); `auto_run_sign_in_proposal`, `auto_run_approve_sign_in_proposal`, `auto_run_reject_sign_in_proposal` commands + `_with` fns |
| `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` | Modify | Register module and commands |
| `src-tauri/src/commands/autorun_check_signin.rs` | Modify | `pub(crate) fn claim_check_browser()` shared by check/inspect/propose |
| `src-tauri/src/commands/autorun_record_signin.rs` | Modify | `for_saving` becomes `pub(crate)` |
| `src-tauri/src/events.rs` | Modify | `AutorunDataKind::SignInProposal` |
| `src-tauri/src/ai_bridge.rs` | Modify | Routes `/autorun-inspect-sign-in`, `/autorun-propose-sign-in`; attempt counting in `check_sign_in_in` |
| `src-tauri/src/mcp.rs` | Modify | Tools `inspect_sign_in_page`, `propose_sign_in_recipe` (schema + dispatch) |
| `src-tauri/src/ai_tools.rs` | Modify | `AUTORUN_TOOLS`, `CLAUDE_AUTORUN_RULES`/`CURSOR_AUTORUN_ENTRIES` (inspect only), guided command bodies |
| `src-tauri/src/autorun/guide.rs` | Modify | New section "When the sign-in page does not fit"; stop rules point to it |
| `src-tauri/src/autorun/roles/{writer,explorer,healer}.md` | Modify | One line each pointing to that section |
| `src/lib/mcpTools.ts` | Modify | Labels + name list |
| `src/hooks/useAutoRunDataSync.ts` | Modify | Key for `signInProposal` |
| `src/screens/AutoRun/SignInProposalNotice.tsx` (+ `.test.tsx`) | Create | The "Proposed by the assistant" notice |
| `src/screens/AutoRun/SetupPanel.tsx` | Modify | Render the notice under the Sign-in row |
| `docs-site/src/content/auto-run.ts`, `src-tauri/help/` | Modify | Record sign-in note; rebuilt site |
| Tests: `src-tauri/tests/suite/{autorun_recipe,autorun_signin,signin_inspect,signin_attempts,signin_proposal,signin_proposal_bridge,autorun_check_signin,tcm_mcp,ai_tools,autorun_guide}.rs` | Create/Modify | As per task |

---

### Task 1: Change-password page without a current-password field

**Files:**
- Modify: `src-tauri/src/autorun/recipe.rs` (`PasswordResetPage` ~132, `validate` ~621)
- Modify: `src-tauri/src/autorun/signin.rs` (`fill_reset_form` ~579)
- Test: `src-tauri/tests/suite/autorun_recipe.rs`, `src-tauri/tests/suite/autorun_signin.rs`

**Interfaces:**
- Produces: `PasswordResetPage { path_contains, field, new, confirm, submit, #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub no_current_password: bool }`. With it set, `field` only recognises the page (often the new-password field) and is never filled; `new` is then required.
- Consumes: `fill_reset_form(d, recipe, account, new, timing, policy)` (unchanged signature).

- [ ] **Step 1: Write failing tests**
  - `autorun_recipe.rs::a_reset_page_without_a_current_field_needs_its_new_field`:
    ```rust
    let mut r = menu_recipe();
    r.password_reset = Some(serde_json::from_value(json!({ "path_contains": "/account/change", "field": { "css": "#newPwd" }, "no_current_password": true })).unwrap());
    assert_eq!(r.validate().unwrap_err(), "password_reset: a page with no current password field names its new password field (new)");
    r.password_reset.as_mut().unwrap().new = Some(serde_json::from_value(json!({ "css": "#newPwd" })).unwrap());
    assert!(r.validate().is_ok());
    // Old files read unchanged and write without the flag.
    let old: PasswordResetPage = serde_json::from_value(json!({ "path_contains": "/x", "field": { "css": "#o" } })).unwrap();
    assert!(!old.no_current_password);
    assert!(!serde_json::to_string(&old).unwrap().contains("no_current_password"));
    ```
  - `autorun_signin.rs::a_reset_without_a_current_field_types_only_the_new_password` (beside `reset_on_test_environment_saves_new_password_and_signs_in`, using `reset_root(true, true)` and `reset_app`): recipe = `recipe_at_login()` with `password_reset = { path_contains: "/security/PasswordReset", field: {css:"#txtNewPassword"}, new: {css:"#txtNewPassword"}, no_current_password: true }`. Assert `out.ok`, `app.typed_into("#txtOldPassword").is_empty()`, `app.typed_into("#txtNewPassword") == ["Abc@123"]`, `app.typed_into("#txtConfirmPassword") == ["Abc@123"]`, `app.submits == 1`, `account_file_password(dir.path()) == "Abc@123"`.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_recipe::a_reset_page_without` then `cargo test --test suite autorun_signin::a_reset_without` - expected FAIL (unknown field / old password typed).
- [ ] **Step 3: Implement** the field and the validate rule (sentence above verbatim, checked after `path_contains`); in `fill_reset_form` build the action list as `[current?, new, confirm]` where current is `fill(&page.field, &account.password)` only when `!page.no_current_password`. Update the `PasswordResetPage` doc comment for `field`.
- [ ] **Step 4: Run** both tests, then `cargo test --test suite autorun_signin::` - expected PASS.
- [ ] **Step 5: Commit** `feat(v2): a change-password page may have no current password field` (recipe.rs, signin.rs, both test files).

### Task 2: Trial sign-in that never resets and never saves a session

**Files:**
- Modify: `src-tauri/src/autorun/signin.rs` (`sign_in_core` ~1047, `expired_at_reset_page` ~662, `try_pending` ~759, `sign_in_held` ~1028)
- Test: `src-tauri/tests/suite/autorun_signin.rs`

**Interfaces:**
- Produces: private `enum Attempt { First, AfterReset, Trial }` replacing `sign_in_core`'s `may_reset: bool` (`First` = old `true`, `AfterReset` = old `false`). `Trial`: on the reset page returns `run.password_expired(d, !reset_enabled(root))` without filling anything; never saves the session; never tries a pending password.
- Produces: `pub async fn sign_in_trial<D: Driver>(d: &mut D, root: &Path, recipe: &SignInRecipe, account: &Account, timing: &Timing) -> SignInOutcome` (no saved session, `Attempt::Trial`, browser words `UNNAMED_BROWSER`, page-error book trimmed like `sign_in_with`).
- Produces: `pub(crate) fn reset_page(recipe: &SignInRecipe) -> Option<PasswordResetPage>` (visibility only).

- [ ] **Step 1: Write failing tests** in `autorun_signin.rs`:
  - `a_trial_on_the_reset_page_never_sends_the_form`: `reset_root(true, true)`, `reset_app(&["Abc@123","Test@1234"])`, `sign_in_trial(&mut d, root, &reset_recipe(false), &reset_account(), &quick())`. Assert `!out.ok`, `out.detail.starts_with(PASSWORD_EXPIRED)`, `app.submits == 0`, `app.typed_into("#txtNewPassword").is_empty()`, `pending_of(root).is_none()`, `account_file_password(root) == PASSWORD`.
  - `a_trial_that_signs_in_saves_no_session`: `stateful_app(false, None)`; `sign_in_trial` with `recipe_at_login()`; assert `out.ok` and `!accounts::session_path(root, &account().key).unwrap().exists()`.
  - `a_trial_ignores_a_held_password`: hold a pending password via `accounts::set_pending_password(root, &env_id, KEY, "Abc@123")`, trial; assert `app.count_of("Abc@123") == 0` and the pending password is still held.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_signin::a_trial` - expected FAIL (`sign_in_trial` missing).
- [ ] **Step 3: Implement** the enum, thread it through `sign_in_core`, `expired_boxed`, `expired_at_reset_page` (`Trial` branch first), and the session save (`if after.is_ok() && attempt != Attempt::Trial`). Add `sign_in_trial`.
- [ ] **Step 4: Run** `cargo test --test suite autorun_signin::` and `cargo test --test suite autorun_check_signin::` - expected PASS (no existing behaviour changes).
- [ ] **Step 5: Commit** `feat(v2): a trial sign-in that never resets a password or keeps a session`.

### Task 3: Unknown change-password page

**Files:**
- Create: `src-tauri/src/autorun/signin_inspect.rs` (first part only); Modify: `src-tauri/src/autorun/mod.rs`
- Modify: `src-tauri/src/autorun/signin.rs` (both `on_reset_page` sites ~1177, ~1188; `is_password_expired` ~407)
- Test: `src-tauri/tests/suite/autorun_signin.rs`

**Interfaces:**
- Produces in `signin_inspect.rs`: `pub const PASSWORD_FIELDS_JS: &str` (an expression evaluating to the number of visible `input[type=password]` elements: has client rects, computed `visibility` not hidden, `display` not none) and `pub async fn visible_password_fields<D: Driver>(d: &mut D) -> usize` (`page::eval_value(d, PASSWORD_FIELDS_JS)`; anything not a number is 0).
- Produces in `signin.rs`: `pub const CHANGE_PASSWORD_UNKNOWN: &str = "password expired - the site showed a change-password page this sign-in does not describe";` `is_password_expired` also true for `detail.starts_with(CHANGE_PASSWORD_UNKNOWN)`. Stop words: `"{CHANGE_PASSWORD_UNKNOWN} - {landing_words}"`, plus `" - {RESET_SWITCH_HINT}"` when `!reset_enabled(root)`.

- [ ] **Step 1: Write failing tests** (fake answering `Runtime.evaluate` with `params["expression"] == PASSWORD_FIELDS_JS`):
  - `an_unknown_change_password_page_is_password_expired`: `cold_app("https://hr.example.internal/account/ChangePassword", "Change password", false, true)` answering 2; recipe whose `password_reset` is `{ "path_contains": "/nowhere", "field": {"css":"#nothing"} }`. Assert `out.detail.starts_with(CHANGE_PASSWORD_UNKNOWN)`, `is_password_expired(&out.detail)`, `out.detail.contains("/account/ChangePassword")`, `out.detail.contains(RESET_SWITCH_HINT)` (temp dir, switch off), `!out.wrong_password`, `typed_count(&d) == 1`, `no_password_anywhere(&out)`.
  - same page with `reset_root(true, true)`: detail does NOT contain `RESET_SWITCH_HINT`.
  - `one_password_field_is_not_a_change_password_page`: `cold_app(LOGIN_URL, "Sign in", true, false)` answering 1; assert `out.detail.starts_with(MARKER_NEVER_APPEARED)`.
  - `a_wait_step_that_stops_on_an_unknown_change_password_page_says_so`: steps ending in a `wait_for` of `#marker`, page answering 2; assert starts with `CHANGE_PASSWORD_UNKNOWN`, not `"sign-in stopped at step"`.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_signin::` - expected FAIL on the four.
- [ ] **Step 3: Implement**: after each `on_reset_page` check that is false and not a harness failure, `if visible_password_fields(d).await >= 2 { return run.change_password_unknown(d, !reset_enabled(root)).await; }` (a `Run` method beside `password_expired`). This runs before `login_form_showing`, so it is never a wrong password.
- [ ] **Step 4: Run** `cargo test --test suite autorun_signin::`, `autorun_check_signin::`, `autorun_failures::` - expected PASS.
- [ ] **Step 5: Commit** `feat(v2): a sign-in names a change-password page its recipe does not describe`.

### Task 4: Page-inspect reader

**Files:**
- Modify: `src-tauri/src/autorun/signin_inspect.rs`
- Test: Create `src-tauri/tests/suite/signin_inspect.rs`; add `mod signin_inspect;` to `suite/main.rs`

**Interfaces:**
- Produces (serde `Serialize` only, not specta; this goes to the assistant):
  ```rust
  pub const INSPECT_JS: &str; // returns { controls: [RawControl], actions: [RawAction] }, visible elements only
  pub const MOST_CONTROLS: usize = 40;
  pub const MOST_ACTIONS: usize = 40;
  #[derive(serde::Deserialize)] pub struct RawControl { pub tag: String, #[serde(rename = "type")] pub kind: String, pub id: String, pub name: String, pub label: String, pub placeholder: String, pub password: bool, pub role: String }
  #[derive(serde::Deserialize)] pub struct RawAction { pub kind: String /* "button" | "link" */, pub text: String, pub id: String }
  #[derive(serde::Serialize)] pub struct PageControl { pub role: String, pub input_type: String, pub id: String, pub name: String, pub label: String, pub placeholder: String, pub password: bool, pub locator: Target }
  #[derive(serde::Serialize)] pub struct PageAction { pub kind: String, pub text: String, pub locator: Target }
  #[derive(serde::Serialize)] pub struct SignInPageView { pub path: String, pub title: String, pub controls: Vec<PageControl>, pub actions: Vec<PageAction>, pub change_password_hint: Option<String>, pub cut: bool }
  pub const CHANGE_PASSWORD_HINT: &str = "this page looks like a change-password page (two or more password fields): describe it as the recipe's password_reset";
  pub fn control_locator(c: &RawControl) -> Target; // signin_recorder::field_css(&FieldHints{..}) as {css}; else {role:"textbox", name: label, exact: true}
  pub fn action_locator(a: &RawAction) -> Target;   // id -> {css:"#id"} via css_escape; else {role: kind, name: text, exact: true}
  pub async fn read_sign_in_page<D: Driver>(d: &mut D) -> Result<SignInPageView, String>;
  pub async fn reset_fields_showing<D: Driver>(d: &mut D, page: &PasswordResetPage, timing: &Timing) -> bool; // path holds path_contains AND field, new, confirm, submit (built-ins filled by signin::reset_page) each visible
  ```
  `INSPECT_JS` reads only `tagName`, `type`, `id`, `name`, label text (`labels`, `aria-label`, `aria-labelledby`), `placeholder`, `role`, button/link `innerText`, and for `input[type=submit|button|reset]` `getAttribute('value')` (the page's own caption, never typed). Text trimmed to 80 chars. `path` via `save_guard::path_of(location.href)`; `title` trimmed like `landing_words`. `cut` when either list hit its cap. `change_password_hint` is `Some(CHANGE_PASSWORD_HINT)` when two or more controls have `password`.

- [ ] **Step 1: Write failing tests** in `signin_inspect.rs`:
  - `inspect_js_never_reads_a_value`: `assert!(!INSPECT_JS.contains(".value"))`, `!PASSWORD_FIELDS_JS.contains(".value")`, and `INSPECT_JS.matches("getAttribute('value')").count() == 1`.
  - `inspect_answer_carries_no_value_even_when_the_page_sends_one`: ScriptedDriver answering `INSPECT_JS` with a control carrying an extra `"value": "s3cret-typed"`; `read_sign_in_page`; `serde_json::to_string(&view)` does not contain `s3cret-typed`.
  - `locators_are_in_the_recipe_target_format`: `control_locator` for `{id:"txtUser"}` serialises to `{"css":"#txtUser"}`; for `{name:"pwd", tag:"input"}` to `{"css":"input[name=\"pwd\"]"}`; for a password field with nothing else to `{"css":"input[type=\"password\"]"}`; `action_locator({kind:"button", text:"Sign in", id:""})` to `{"role":"button","name":"Sign in","exact":true}`; each parses back with `serde_json::from_value::<Target>` and `.validate()` is Ok.
  - `two_password_fields_give_the_hint_and_the_path_has_no_host`: location `https://hr.example.internal/account/ChangePassword?x=1`; `view.path == "/account/ChangePassword"`, `view.change_password_hint.as_deref() == Some(CHANGE_PASSWORD_HINT)`, serialized view has no `hr.example.internal`.
  - `forty_one_controls_are_cut_to_forty`: `view.controls.len() == 40 && view.cut`.
  - `reset_fields_showing_needs_every_field`: page on `/security/PasswordReset` with `#txtOldPassword`, `#txtNewPassword`, `#txtConfirmPassword`, `#btnsubmit` visible -> true; drop `#btnsubmit` -> false.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite signin_inspect::` - expected FAIL (missing items).
- [ ] **Step 3: Implement** the items above (make `signin_recorder::css_escape` `pub(crate)` if it is not).
- [ ] **Step 4: Run** `cargo test --test suite signin_inspect::` - expected PASS.
- [ ] **Step 5: Commit** `feat(v2): read a sign-in page's controls for the assistant, never their values`.

### Task 5: Sign-in attempt counter

**Files:**
- Create: `src-tauri/src/autorun/signin_attempts.rs`; Modify `autorun/mod.rs`, `tests/suite/serial.rs` (`pub fn sign_in_attempts() -> MutexGuard<'static, ()>`)
- Test: Create `src-tauri/tests/suite/signin_attempts.rs`; `mod signin_attempts;` in `main.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const MOST_SIGN_IN_ATTEMPTS: u32 = 3;
  pub fn attempts_used(account: &str) -> String; // "the account \"{account}\" has had 3 sign-in attempts for this sign-in problem - report it to the person and stop; do not try again"
  pub fn take(root: &Path, key: &str) -> Result<u32, String>; // opens or continues the problem; Err(attempts_used) once 3 are used
  pub fn take_if_open(root: &Path, key: &str) -> Result<bool, String>; // counts only when a problem is open
  pub fn is_open(root: &Path, key: &str) -> bool;
  pub fn close(root: &Path, key: &str); // a check that signed the account in
  pub fn forget_all(); // for tests
  ```
  Memory only (a `static Mutex<BTreeMap<String, u32>>` keyed `"{root}|{key}"`, like `signin::REFUSED`): one app session.

- [ ] **Step 1: Write failing test** `three_attempts_then_refused_and_a_close_starts_over` (takes `serial::sign_in_attempts()`, calls `forget_all()`): `take` returns `Ok(1)`, `Ok(2)`, `Ok(3)`, then `Err(attempts_used("sup"))`; `take_if_open` for another key is `Ok(false)` and leaves it closed; `close` then `take` is `Ok(1)`; another data folder counts apart.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite signin_attempts::` - FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** - PASS.
- [ ] **Step 5: Commit** `feat(v2): count the assistant's sign-in attempts per account`.

### Task 6: Proposal store, merge rule, approve and reject

**Files:**
- Create: `src-tauri/src/autorun/signin_proposal.rs`; Modify `autorun/mod.rs`
- Create: `src-tauri/src/commands/autorun_signin_proposal.rs` (commands part); Modify `commands/mod.rs`, `lib.rs` (register three commands), `commands/autorun_record_signin.rs` (`pub(crate) fn for_saving`), `events.rs` (`AutorunDataKind::SignInProposal`)
- Test: Create `src-tauri/tests/suite/signin_proposal.rs`; `mod signin_proposal;` in `main.rs`

**Interfaces:**
- Produces in `signin_proposal.rs`:
  ```rust
  #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  pub struct StoredSignInProposal { pub recipe: SignInRecipe /* merged, as it would be saved */, pub account: String, pub proposed_ms: f64, pub summary: String, pub saved_hash: String, pub changes: Vec<String> }
  pub fn proposal_path(root: &Path, env_id: &str, org: &str, project: &str) -> PathBuf; // shared_of(root)/proposals/signin-{env_id}-{project_slug}.json
  pub fn saved_hash(root: &Path, org: &str, project: &str) -> Result<String, String>; // "" when no recipe file, else format!("{:08x}", recipe::fnv1a(&text))
  pub fn merge(draft: &SignInRecipe, saved: Option<&SignInRecipe>) -> SignInRecipe; // draft's start_url, steps, signed_in, signed_in_in_menu, password_reset; rest from saved; no saved -> draft whole
  pub fn changed_parts(merged: &SignInRecipe, saved: Option<&SignInRecipe>) -> Vec<String>; // of "address","steps","signed-in check","change-password part"
  pub fn summary(merged: &SignInRecipe) -> String;
  pub const RESET_PART_WORDS: &str = "the change-password part was found on the page but not sent; it is used the next time an expired password is met";
  pub fn save(root: &Path, env_id: &str, org: &str, project: &str, p: &StoredSignInProposal) -> Result<(), String>; // replaces any earlier; notifies SignInProposal
  pub fn load(root: &Path, env_id: &str, org: &str, project: &str) -> Result<Option<StoredSignInProposal>, String>; // parse errors never quoted
  pub fn clear(root: &Path, env_id: &str, org: &str, project: &str) -> Result<(), String>; // notifies
  #[derive(Debug, Clone, serde::Serialize, specta::Type)]
  pub struct SignInProposalView { pub address: String /* path only, or "this environment's address" */, pub steps: Vec<String>, pub signed_in: String, pub signed_in_in_menu: bool, pub password_reset: Option<String>, pub account: String, pub changes: Vec<String>, pub offer_reset_switch: bool }
  pub fn view(p: &StoredSignInProposal, env: &Environment) -> SignInProposalView; // offer_reset_switch = p.recipe.password_reset.is_some() && env.test_environment && !env.reset_expired_passwords
  pub const PROPOSAL_STALE: &str = "the sign-in recipe changed after the assistant proposed this one, so it was not saved - ask the assistant to try again";
  pub const RESET_SWITCH_NOT_TEST: &str = "Reset expired passwords can only be turned on for a test environment - nothing was saved";
  ```
  Step words: `report::action_words`, except `navigate` says `go to <path>` and `fill` says `fill in <locator> with the username` / `with the password` / `with fixed text` (never the text). `password_reset` words: `"page <path_contains>: new password <locator>, confirm <locator>, send <locator>"` plus `", current password <field>"` unless `no_current_password`.
- Produces in `commands/autorun_signin_proposal.rs`:
  ```rust
  pub fn proposal_view_with(root: &Path, org: &str, project: &str) -> Result<Option<SignInProposalView>, String>;
  pub fn approve_with(root: &Path, org: &str, project: &str, turn_on_reset: bool) -> Result<(), String>;
  pub fn reject_with(root: &Path, org: &str, project: &str) -> Result<(), String>;
  #[tauri::command] #[specta::specta] pub fn auto_run_sign_in_proposal(app: tauri::AppHandle, organization: String, project: String) -> Result<Option<SignInProposalView>, String>;
  #[tauri::command] #[specta::specta] pub fn auto_run_approve_sign_in_proposal(app: tauri::AppHandle, organization: String, project: String, turn_on_reset: bool) -> Result<(), String>;
  #[tauri::command] #[specta::specta] pub fn auto_run_reject_sign_in_proposal(app: tauri::AppHandle, organization: String, project: String) -> Result<(), String>;
  ```
  `approve_with` order: load (none -> `"there is no proposed sign-in to approve"`); hash differs -> `PROPOSAL_STALE` (proposal kept); `turn_on_reset` on a non-test env -> `RESET_SWITCH_NOT_TEST`; `recipe.validate()`; `save_recipe(root, org, project, &for_saving(root, org, project, recipe)?)`; then `environments::set_reset_expired_passwords(root, &env.id, true)` only when `turn_on_reset && recipe.password_reset.is_some()`; `clear`; log `"Auto-run sign-in recipe approved from the assistant's proposal"`.

- [ ] **Step 1: Write failing tests** in `signin_proposal.rs` (temp roots built like `reset_root` in `autorun_signin.rs`):
  - `merge_takes_only_the_five_parts_from_the_draft`: saved has `after_sign_in` 1 step, `session_minutes: 30`, `allowed_origins: ["https://sso.example.test"]`, a `close_when_shown` entry; draft has other steps, `signed_in_in_menu: true`, a `password_reset`, `session_minutes: 999`. Assert merged steps/signed_in/in_menu/password_reset/start_url are the draft's and `after_sign_in`, `session_minutes == 30`, `allowed_origins`, `close_when_shown` are the saved's; `merge(&draft, None) == draft`.
  - `a_new_proposal_replaces_the_earlier_one`: save A then B; `load` gives B; file name ends `signin-{env_id}-{slug}.json` under `shared_of(root)/proposals`.
  - `approve_refuses_when_the_saved_recipe_changed`: propose with hash of recipe R1, then `save_recipe` R2; `approve_with` is `Err(PROPOSAL_STALE)`; `load_recipe` still R2; proposal still there.
  - `approve_keeps_the_environments_own_address`: env `start_url = "https://env.example.test"`, saved recipe `start_url = "https://saved.example.test"`, proposal recipe `start_url = "https://env.example.test"`; after approve `load_recipe(...).start_url == "https://saved.example.test"` and steps are the proposal's.
  - `approve_turns_the_switch_on_only_on_a_test_environment`: non-test env + `turn_on_reset: true` -> `Err(RESET_SWITCH_NOT_TEST)` and no recipe written; test env -> `Ok`, `env.reset_expired_passwords == true`, proposal cleared; test env + `false` -> switch stays off.
  - `reject_clears_and_saves_nothing`.
  - `the_view_names_no_host_and_no_fixed_text`: recipe with a navigate to `https://hr.example.internal/login?x=1` and a fill of fixed text `"Branch-7"`; `serde_json::to_string(&view)` contains `/login`, not `hr.example.internal`, not `Branch-7`; `offer_reset_switch` true only for test env with switch off and a `password_reset`.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite signin_proposal::` - FAIL.
- [ ] **Step 3: Implement** both files, register the commands in `lib.rs` beside `autorun_record_signin::*`, add the event kind.
- [ ] **Step 4: Run** `cargo test --test suite signin_proposal::` then `cargo test --test bindings` (regenerates `src/bindings.ts`, token gate) - PASS.
- [ ] **Step 5: Commit** `feat(v2): keep one proposed sign-in per environment and approve or reject it` (include `src/bindings.ts`).

### Task 7: Bridge handlers, routes and MCP tools

**Files:**
- Modify: `src-tauri/src/commands/autorun_signin_proposal.rs` (handlers), `commands/autorun_check_signin.rs`, `ai_bridge.rs` (routes ~825, `check_sign_in_in` ~12459), `mcp.rs` (schemas near `check_autorun_sign_in` ~965, dispatch ~2754)
- Test: Create `src-tauri/tests/suite/signin_proposal_bridge.rs` (`mod` line); Modify `tcm_mcp.rs`, `autorun_check_signin.rs`

**Interfaces:**
- Produces in `autorun_check_signin.rs`: `pub(crate) fn claim_check_browser() -> Result<crate::commands::autorun_replay::OneAtATime, String>` (claim, then `others_busy`; same sentences as `check_with`, which now uses it).
- Produces in `commands/autorun_signin_proposal.rs`:
  ```rust
  pub const INSPECT_SHAPE: &str = "{ \"account\"?: <a saved account key>, \"recipe\"?: <a draft sign-in recipe> } - both or neither";
  pub const PROPOSE_SHAPE: &str = "{ \"recipe\": <the whole sign-in recipe>, \"account\": <a saved account key> }";
  pub async fn inspect_in<B: Browsers>(browsers: &mut B, root: &Path, org: &str, project: &str, body: &str, timing: &Timing) -> (u16, String);
  pub async fn propose_in<B: Browsers>(browsers: &mut B, root: &Path, org: &str, project: &str, body: &str, timing: &Timing) -> (u16, String);
  ```
  `inspect_in`: busy (`refusal()`) -> 409; parse (own sentence + shape, never serde's words) -> 400; only one of account/recipe -> 400; unsaved key -> 400 naming it; with account: `signin_attempts::take` (Err -> 409), `recipe.validate()`, lease `Held::setup().hold`, open browser, `sign_in_trial(effective_recipe(&draft, &env))`, then `read_sign_in_page`; without: `load_effective_recipe_if_any` (None -> 400 `NO_SITE_ADDRESS`), navigate under `Policy::only(origins)` (never signs in, never counts), read. Answer `200 {"page": SignInPageView, "sign_in"?: <redacted outcome words>}`. Browser closed, lease let go on every path.
  `propose_in`: busy -> 409; parse -> 400; unsaved key -> 400; `take` -> 409; `draft.validate()`; `merged = merge(&draft, saved)`; `merged.validate()`; `check_marker_for(&merged.signed_in, &who)`; claim; lease; open; `effective = effective_recipe(&for_saving(..merged)?, &env)`; `refuse_check_on_start_page(d, &effective)`; `sign_in_trial`. Pass = `out.ok`, or `merged.password_reset.is_some() && is_password_expired(&out.detail) && reset_fields_showing(d, page, timing)`. On pass: `signin_proposal::save` (hash taken BEFORE the trial) and answer `200 {"proposed": true, "summary": ...}` (summary ends with `RESET_PART_WORDS` when a reset part is included). On fail: `422 "the trial did not sign in - nothing was proposed: {redact(detail)}"` (`check_failure` style for harness / page did not open), nothing stored.
- Produces in `ai_bridge.rs`: `("POST", "/autorun-inspect-sign-in")` and `("POST", "/autorun-propose-sign-in")` calling those with `RealBrowsers::new(Browser::from_name(&last_browser(&root)), false)` and `Timing::supervised()`; `check_sign_in_in` calls `signin_attempts::take_if_open` per key before any browser (Err -> 409) and `close` for each key that `signed_in`.
- Produces in `mcp.rs`: tools `inspect_sign_in_page` (properties `account`: string, `recipe`: object; none required) and `propose_sign_in_recipe` (`recipe`: object, `account`: string; both required), dispatch `call("POST", "/autorun-inspect-sign-in" | "/autorun-propose-sign-in", &args.to_string())`. Descriptions say: never shows a typed value; refusals as `check_autorun_sign_in`; nothing is saved until the person approves in TCM; trial never sends a change-password form; 3 attempts per account per problem. No dashes U+2014/U+2013.

- [ ] **Step 1: Write failing tests**
  - `signin_proposal_bridge.rs` (fake `Browsers` like `Fake` in `autorun_check_signin.rs`; takes `serial::sign_in_attempts()` and `serial::account_leases()`):
    - `inspect_without_an_account_never_signs_in`: answer 200, page path present, `typed_count == 0`, `signin_attempts::is_open(root, "sup") == false`.
    - `inspect_refuses_an_unsaved_account_before_a_browser_opens`: 400 naming `"ghost"`, `fake.opened == 0`.
    - `a_proposal_that_signs_in_is_stored_and_says_so`: 200, `"proposed":true`; `signin_proposal::load` is `Some` with `account == "sup"`; `load_recipe` unchanged (nothing saved).
    - `a_failed_trial_stores_nothing`: 422 starting `"the trial did not sign in - nothing was proposed"`; no proposal file; answer has no `PASSWORD` and no host.
    - `a_reset_part_passes_on_the_described_page_without_sending`: `reset_root(true, true)`-style root, fake on the reset page with all four fields; 200; summary contains `RESET_PART_WORDS`; submits 0; stored password unchanged.
    - `a_marker_naming_the_account_is_refused`: `signed_in` text containing the username -> 400 with `check_marker_for`'s sentence, `fake.opened == 0`.
    - `a_parse_error_never_quotes_the_body`: body with `"steps": "hunter2"` -> 400 not containing `hunter2`.
  - `autorun_check_signin.rs`:
    - `the_follow_up_check_counts_and_the_fourth_attempt_is_refused`: `take` twice for "sup", one failing check (counted, 3), next `check_sign_in_in` -> 409 with `attempts_used("sup")` and no browser opened.
    - `a_check_with_no_open_problem_is_not_counted`: four checks in a row all run (no 409).
    - `a_check_that_signs_in_closes_the_problem`.
  - `tcm_mcp.rs::the_sign_in_proposal_tools_are_listed_and_reach_their_routes` modelled on `the_setup_tools_are_listed_and_reach_their_routes` with `[("inspect_sign_in_page", "/autorun-inspect-sign-in"), ("propose_sign_in_recipe", "/autorun-propose-sign-in")]`, minus the `agent` property assertion, plus "absent with Auto Run off".
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite signin_proposal_bridge::`, then `autorun_check_signin::`, then `tcm_mcp::the_sign_in` - FAIL.
- [ ] **Step 3: Implement** as specified.
- [ ] **Step 4: Run** the three again, then `cargo test --test suite tcm_mcp::` and `ai_tools::` (tool parity tests may need Task 8's list entries; if `every_tool_gets_a_command_and_each_describes_itself` fails, do Task 8 Step 3's `AUTORUN_TOOLS` change here) - PASS.
- [ ] **Step 5: Commit** `feat(v2): the assistant can inspect a sign-in page and propose a tested recipe`.

### Task 8: Allow-lists, labels, guided commands and guide text

**Files:**
- Modify: `src-tauri/src/ai_tools.rs` (`AUTORUN_TOOLS` ~157; `CLAUDE_AUTORUN_RULES`, `CURSOR_AUTORUN_ENTRIES` ~1492-1518; command bodies setup 388, discover 459 step 2, map-menus 544 steps 2 and 5, heal 588, gen-tests 688 ~767, explore 790 step 2)
- Modify: `src-tauri/src/autorun/guide.rs` (Quick rules ~188; Environments ~1252-1270; Discovering the app ~1948; ~2060; Repairing STOP line ~2827), `src-tauri/src/autorun/roles/{writer,explorer,healer}.md`
- Modify: `src/lib/mcpTools.ts` (~216-230 labels, ~388-390 list)
- Test: `src-tauri/tests/suite/autorun_guide.rs`, `src-tauri/tests/suite/ai_tools.rs`, `src/lib/mcpTools.test.ts`

**Interfaces:**
- Produces: guide section `## When the sign-in page does not fit` (placed after `## Environments`) holding the one shared rule, verbatim in substance from the spec: mismatched page (a step failed, or the check never appeared and the form is not still showing) -> `end_autorun_discovery`, `inspect_sign_in_page`, build a recipe, `propose_sign_in_recipe` with a different saved account than the one that failed when one exists, tell the person to approve it in TCM (Auto Run, Setup, Sign-in) and wait, then `check_autorun_sign_in`, then a fresh `start_autorun_discovery` at the same area. Unknown change-password page ("does not describe") -> inspect with `account` + saved recipe, propose with `password_reset` (`no_current_password` when the page has no current field), ask to approve and to tick Also turn on Reset expired passwords on a test environment, sign in again. Limits: 2 proposals, 3 attempts per account, and the hard stops (rejected proposal, second failed trial, wrong password, locked account, captcha, one-time code, a page needing more than one form).
- Produces: `AUTORUN_TOOLS` gains both names after `check_autorun_sign_in`; `CLAUDE_AUTORUN_RULES` gains `"mcp__tcm__inspect_sign_in_page"`, `CURSOR_AUTORUN_ENTRIES` gains `"tcm:inspect_sign_in_page"` (read-only; propose stays prompted). `mcpTools.ts`: `{ name: "inspect_sign_in_page", label: "Inspect a sign-in page", summary: "Read a sign-in or change-password page's fields, buttons and links, never what is typed in them." }`, `{ name: "propose_sign_in_recipe", label: "Propose a sign-in", summary: "Try a sign-in recipe with a saved account and leave it for you to approve or reject in Auto Run, Setup." }`, both in the Auto Run name list.

- [ ] **Step 1: Write failing tests**
  - `autorun_guide.rs::the_sign_in_rule_is_shared_and_no_longer_just_stops`: the section exists and contains `inspect_sign_in_page`, `propose_sign_in_recipe`, `end_autorun_discovery`, `at most 2 proposals`, `3 sign-in attempts`, `does not describe`, `Also turn on Reset expired passwords`, `captcha`, `one-time code`; "Discovering the app" no longer contains `If the sign-in fails, report it and stop` and does contain `When the sign-in page does not fit`; Quick rules mention the section; no `\u{2014}`/`\u{2013}` in any of them.
  - `ai_tools.rs::live_app_commands_point_to_the_sign_in_rule`: for stems `discover`, `map-menus`, `explore`, `gen-tests`, `heal`, the rendered body contains `When the sign-in page does not fit`; `setup` body contains `propose_sign_in_recipe` as an alternative to Record sign-in; map-menus still lists `wrong password, a locked account, a captcha or a one-time code` as stops; and `AUTORUN_TOOLS` contains both tools; `CLAUDE_AUTORUN_RULES` contains the inspect rule only.
  - Role check in `ai_tools.rs::agent_rules_name_the_sign_in_rule`: `writer.md`, `explorer.md`, `healer.md` each contain `When the sign-in page does not fit`; `healer.md` no longer says `Stop at once on a login page` without it.
  - `src/lib/mcpTools.test.ts`: both names are labelled and in the Auto Run list (extend the existing parity test if it compares against a Rust list).
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_guide::the_sign_in_rule`, `cargo test --test suite ai_tools::live_app`, `cargo test --test suite ai_tools::agent_rules`, `npx vitest run src/lib/mcpTools.test.ts` - FAIL.
- [ ] **Step 3: Implement** the texts and lists. The discover step 2 ending becomes "If the sign-in fails, follow the guide's \"When the sign-in page does not fit\"." and likewise for the others; healer's line becomes "Stop at once on a missing precondition or a product defect; on a login page, follow the guide's \"When the sign-in page does not fit\"".
- [ ] **Step 4: Run** `cargo test --test suite autorun_guide::`, `ai_tools::`, `tcm_mcp::`, `allow_rules::`, then `npx vitest run src/lib/mcpTools.test.ts` - PASS.
- [ ] **Step 5: Commit** `feat(v2): every live-app command recovers from a sign-in page that does not fit`.

### Task 9: Proposal notice in the app

**Files:**
- Create: `src/screens/AutoRun/SignInProposalNotice.tsx`, `src/screens/AutoRun/SignInProposalNotice.test.tsx`
- Modify: `src/screens/AutoRun/SetupPanel.tsx` (after the Sign-in `SetupRow` ~761), `src/hooks/useAutoRunDataSync.ts` (`signInProposal: ["autorun-signin-proposal"]`)

**Interfaces:**
- Consumes: `commands.autoRunSignInProposal(org, project)`, `commands.autoRunApproveSignInProposal(org, project, turnOnReset)`, `commands.autoRunRejectSignInProposal(org, project)`, type `SignInProposalView` (generated in Task 6).
- Produces: `export default function SignInProposalNotice({ org, project }: { org: string; project: string }): JSX.Element | null` - query key `["autorun-signin-proposal", org, project]`; renders nothing with no proposal; a `section` with `aria-label="Proposed sign-in"` and heading "Proposed by the assistant"; lines for address, each step (an ordered list), signed-in check with " (inside a menu)" when `signed_in_in_menu`, change-password part, "Tried with <account>"; each changed part marked "changes" (`Badge`); a `Checkbox` `ariaLabel="Also turn on Reset expired passwords"` only when `offer_reset_switch`, unticked; buttons Approve (`IconConfirm`) and Reject (`IconClear`). Approve/Reject invalidate the proposal key and `["autorun-recipe", org, project]`, and `envKeys.list` (or the environments key used by `useEnvironments`) after a switch change; errors via `toast` with the Rust sentence.

- [ ] **Step 1: Write failing tests** in `SignInProposalNotice.test.tsx` (mock `commands` as `RecordSignInDialog.test.tsx` does):
  - renders nothing when the command answers `null`;
  - shows the steps, `(inside a menu)`, `Tried with sup`, and a `changes` mark on "Steps";
  - no tick box when `offer_reset_switch` is false; with it true the box is shown unticked, and Approve after ticking calls `autoRunApproveSignInProposal("acme", "Web", true)`; without ticking, `false`;
  - Reject calls `autoRunRejectSignInProposal("acme", "Web")`;
  - an Approve that answers `PROPOSAL_STALE`'s sentence shows it in a toast and keeps the notice.
- [ ] **Step 2: Run** `npx vitest run src/screens/AutoRun/SignInProposalNotice.test.tsx` - FAIL.
- [ ] **Step 3: Implement** the component (theme tokens only, layout following the Accounts dialog's Proposed section) and render `<SignInProposalNotice org={org} project={project} />` in `SetupPanel` when `setupReady`.
- [ ] **Step 4: Run** the new test, then `npx vitest run src/screens/AutoRun/` , `npx vitest run src/ui-consistency.test.ts`, `npx tsc --noEmit` - PASS.
- [ ] **Step 5: Commit** `feat(v2): approve or reject the assistant's proposed sign-in in Setup`.

### Task 10: Help guide note

**Files:**
- Modify: `docs-site/src/content/auto-run.ts` (`record-sign-in` entry ~761), rebuilt `src-tauri/help/`
- Test: `docs-site/src/guard.test.ts`, `docs-site/src/built.test.ts` (existing guards)

- [ ] **Step 1:** Append to the `record-sign-in` `does` text: "Your assistant can also work out a sign-in page it does not know: it tries a sign-in with one of your accounts and, if it works, a **Proposed by the assistant** note appears under this row with the steps, the signed-in check and any change-password part. **Approve** saves it as this environment's sign-in; **Reject** drops it. Nothing is saved until you approve. On a test environment the note can also offer **Also turn on Reset expired passwords**."
- [ ] **Step 2: Run** `npx vitest run docs-site/src/built.test.ts` - expected FAIL (built page stale).
- [ ] **Step 3:** `npm run docs:build`. No screenshot changes: the note shows only while a proposal is pending, and the docs shots use sample data with none (if `npm run docs:shots -- --dry-run` reports a moved position for the Setup shot, follow the CLAUDE.md shot procedure).
- [ ] **Step 4: Run** `npx vitest run docs-site/src` - PASS (guard term list unaffected).
- [ ] **Step 5: Commit** `docs(v2): the help guide explains approving the assistant's proposed sign-in` (content file + `src-tauri/help/`).

### Task 11: Full gate

- [ ] `cd src-tauri && cargo test --tests` (one run; wait for it).
- [ ] `npm test`.
- [ ] `npx tsc --noEmit`.
- [ ] `git status` shows nothing unstaged except `.test-cases/`; no commit needed unless a fix was required (then commit that fix by name).

---

## Self-review

- **Spec coverage:** inspect tool (Tasks 4, 7), propose with validate / marker / start page / trial in fresh browser with `effective_recipe` (7), reset-part trial never sends and passes on described fields (2, 4, 7), store with hash and replace (6), approve / stale refusal / `for_saving` / reject (6), notice with steps, in-menu mark, change-password part, account, conditional tick box (9), unknown change-password sentence + `RESET_SWITCH_HINT` + `is_password_expired` (3), current-password optional (1), attempt cap 3 incl. follow-up check (5, 7), guide / commands / agent rules / map-menus hard stops / setup alternative (8), allow-lists and labels (8), help note (10). Changelog excluded by instruction.
- **Step scan:** every task has failing test, run, implement, run, commit; commands are exact.
- **Type consistency:** `SignInRecipe`, `PasswordResetPage.no_current_password`, `sign_in_trial`, `StoredSignInProposal`, `SignInProposalView`, `signin_attempts::{take, take_if_open, close}`, routes and tool names are spelled the same in every task.
- **Review focus:** each of the five failure modes has a named test in its owning task (2, 4, 6, 7, 3).
- **Proportion:** signatures and pinned values only; bodies only where the rule is not obvious (merge, trial verdict, approve order).

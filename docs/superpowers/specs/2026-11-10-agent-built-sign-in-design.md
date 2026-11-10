# Agent-built sign-in and expired-password reset in discovery — design

Date: 2026-11-10. Status: draft for the owner's approval.

## Intent and why

Two owner requests:

1. "Add something to help figure out the login method (sign-in recipe) if page
   is different from usual, so an agent can inspect a login page and create a
   sign in recipe for it to adapt to all login forms." When `/tcm:discover`
   meets a sign-in page the saved recipe does not fit, the agent should work
   the page out and build a recipe for it, not get stuck.
2. "Enable discovery to reset password rather than stopping, and then
   auto-update the password in the accounts section."

Today both end the same way. Discovery's rule is "if the sign-in fails, report
it and stop" (`ai_tools.rs` discover step 2, `guide.rs` ~1948, ~2060, ~2779).
The person must then record a sign-in by hand or reset the password
themselves. The reset machinery already exists (`signin.rs`
`expired_at_reset_page`, `accounts::set_pending_password` /
`commit_pending_password`). It runs only when both of these hold:

- the environment is a test environment with **Reset expired passwords** on;
- the recipe describes the site's change-password page (`password_reset`).

Sites such as the hSenid legacy HRM use their own change-password page, which
no recipe describes, so the reset never starts.

## Owner decisions (2026-11-10)

- **Approve first.** The assistant never saves a sign-in recipe, or the
  change-password part of one, by itself. It proposes, and TCM saves only when
  the person approves in the app.
- **Approach A for both features.** The assistant gets a page-inspecting tool,
  builds the recipe, TCM tests it with a saved account, and the person
  approves it.
- **The test-environment safeguard stays.** Resets still happen only on a test
  environment with Reset expired passwords on.
- **During `/tcm:discover`,** a sign-in page the saved recipe does not fit is
  treated as "the site's sign-in does not match this environment's recipe".
  The agent proposes a **replacement** recipe for the active environment.
  Several sign-ins per project (a second login inside the app) is out of
  scope.

## Goals

1. **Inspect a sign-in page.** The assistant can look at a sign-in page and
   see its fields, buttons and links: what each is, how to find it, whether it
   is a password field. It never sees any typed value.
2. **Propose a recipe.** The assistant can hand TCM a recipe built from that
   view. TCM runs the same checks a recorded recipe gets, then a trial sign-in
   with a saved account in a fresh browser. Only a recipe that signs in
   becomes a proposal.
3. **Approve or reject in the app.** The person sees the proposal (address,
   steps in words, signed-in check, change-password part if any) and approves
   or rejects it. Approving saves it as this environment's recipe, exactly as
   a recorded one would be saved.
4. **Discovery recovers.** `/tcm:discover`, `/tcm:setup` and the Auto Run
   guide tell the agent to inspect and propose on a sign-in failure that looks
   like a mismatched page, instead of stopping.
5. **An unknown change-password page is recognised.** A sign-in that lands on
   a page that looks like a change-password page, but that no recipe
   describes, reports so in its own sentence. The agent can then inspect that
   page and propose the change-password part.
6. **Accounts are updated.** Once that part is approved and the switch is on,
   the next sign-in resets the password through the existing machinery. A
   successful sign-in with the new password then makes it the account's saved
   password, as today.

## Non-goals

- Several sign-in recipes per project, or a second login inside the app.
- Removing the test-environment safeguard for resets.
- The assistant typing credentials itself. TCM fills `{{username}}` and
  `{{password}}` from saved accounts, as today.
- Changing Record sign-in, or how recorded recipes are checked.
- Captchas, one-time codes, or third-party sign-in providers. The assistant
  reports these and stops, as today.

## Anti-goals (a failure even if it works)

- A recipe or change-password part saved without the person's approval.
- A password, or any typed value, reaching the assistant, the chat, a log line
  or the webview.
- A password changed on a non-test environment, or changed during a "trial".
- The agent looping: proposing over and over, or retrying sign-ins in a loop.
- A proposal that replaces this environment's address. An environment with
  its own address keeps it, as with recordings (`for_saving`).

## Approaches considered

- **A. Inspect, test, propose (chosen).** The assistant inspects the page and
  writes the recipe. TCM tests it and shows a proposal for approval. It
  handles unusual pages, and nothing is saved unapproved. It is the most work.
- **B. Built-in guesser.** A button guesses fields with fixed rules. It fails
  on exactly the unusual pages this is for.
- **C. Agent writes JSON, person pastes it.** Cheap, but nothing is tested
  before saving, and it is clumsy mid-discovery.

For the reset: **A. the agent proposes the missing change-password part
(chosen)**, versus **B. also drop the test-environment safeguard**, which was
rejected because an agent changing passwords on a live site is hard to undo.

## Design

### New assistant tools (MCP, through the AI bridge)

**`inspect_sign_in_page`**

- **Input:** `{ account?: <saved key>, recipe?: <draft recipe> }`.
- **No input:** opens the active environment's address, or the saved recipe's
  `start_url` when the environment has none, in a fresh headless check browser
  and reads that page.
- **With `account` and `recipe`:** runs the draft's sign-in steps with that
  account, waits for the page to settle, and reads where it landed. This is
  how the assistant reaches a change-password page, or checks what follows the
  first page of a two-page sign-in.
- **Output:**
  - the page path (never the full address) and title;
  - for each visible form control: role or tag, input type, id, name, label,
    placeholder, whether it is a password field, and a ready locator in the
    recipe's target format;
  - buttons and links with their text and locator;
  - a hint when the page looks like a change-password page (two or more
    password fields).
- **Never returns** any field's value.
- **Same refusals as `check_autorun_sign_in`:** busy, unsaved account key, and
  the environment's allowed sites only. It uses an account lease like that
  tool.

**`propose_sign_in_recipe`**

- **Input:** `{ recipe: <full recipe>, account: <saved key> }`.
- **TCM runs, in order:**
  1. `SignInRecipe::validate`.
  2. `check_marker_for`, which refuses a signed-in check that names the
     account.
  3. `refuse_check_on_start_page`.
  4. A trial sign-in in a fresh browser with that account, with the
     environment's address and allowed sites applied as `effective_recipe`
     does.
- **On success** it stores the proposal for the active environment. Any
  earlier pending proposal is replaced.
- **It answers** `{ proposed: true, summary }`. The summary is plain words:
  what the steps do, what the check is, and whether a change-password part is
  included.
- **On failure** it answers with the same sentences a sign-in check gives, and
  nothing is stored.
- **A recipe that includes `password_reset`:** the trial never sends the
  change-password form. The trial passes when either:
  - the trial signs in; or
  - the trial lands on a page whose path contains `path_contains` and has the
    described fields.

  The summary says the change-password part "was found on the page but not
  sent; it is used the next time an expired password is met".
- **A proposal that only adds or changes `password_reset`** keeps the rest of
  the saved recipe. The assistant sends the whole recipe either way, and the
  person sees what changes.

### Pending proposal store

- One pending sign-in proposal per environment and project, stored beside the
  account proposals (`shared/proposals/`, new file name per environment).
- Each proposal holds: the recipe, the account it was tried with, when, the
  summary, and the saved recipe's content hash at proposal time.
- **Approve:**
  - re-validates the recipe;
  - saves it with `for_saving` / `save_recipe`, so an environment with its own
    address keeps it;
  - clears the proposal.
- **If the saved recipe changed since the proposal,** Approve says so and asks
  the person to have the assistant try again rather than overwrite it.
- **Reject** clears the proposal.

### In the app

- Where the sign-in recipe is shown and recorded, a **"Proposed by the
  assistant"** notice appears while a proposal is pending. It mirrors the
  Accounts dialog's Proposed tab.
- The notice shows:
  - the address (or "this environment's address");
  - each step in words;
  - the signed-in check, marked "(inside a menu)" when `signed_in_in_menu` is
    set;
  - the change-password part, if any;
  - which account it was tried with.
- It has two buttons: Approve and Reject.
- **When the proposal includes a change-password part and the environment is
  a test environment with the switch off,** the notice offers a tick box:
  "Also turn on Reset expired passwords". It is shown only on test
  environments, and is unticked by default.
- No secrets, URLs or paths in the text beyond what the recipe view already
  shows. Theme tokens only.

### Unknown change-password page

- **Where:** in `sign_in_core`, wherever `on_reset_page` is checked today
  (after a failed step and after a missing marker).
- **What is checked:** if no described reset page matched, check whether the
  page looks like a change-password page: two or more visible password
  fields, and no visible username field.
- **If it does,** report a new sentence, for example: "the site showed a
  change-password page this sign-in does not describe". When the reset switch
  is off, `RESET_SWITCH_HINT` is added.
- **Effect on accounts:** `is_password_expired` treats the sentence like
  `PASSWORD_EXPIRED`, so expired-account marks and ranking still work.

### Guided commands and guide text

- **`discover` step 2 and `guide.rs`** (the sign-in stop rules at ~1948,
  ~2060, ~1252-1270, ~2779) change from "report it and stop". The new rule:
  - **Mismatched sign-in page** (a sign-in step failed, or the signed-in check
    never appeared and the sign-in form is not still showing a wrong-password
    state):
    1. Call `inspect_sign_in_page`.
    2. Build a recipe.
    3. Call `propose_sign_in_recipe`.
    4. Tell the person in chat to approve it in TCM, and wait for their reply.
    5. On approval, run `check_autorun_sign_in` and carry on from where
       discovery was.
  - **Unknown change-password page:**
    1. Inspect it with `account` and the saved recipe.
    2. Propose the recipe with a `password_reset` part.
    3. Ask the person to approve it and, when needed, turn the switch on.
    4. Sign in again. The reset runs and Accounts are updated.
  - **Limits:** at most two proposals per sign-in problem in one session. A
    rejected proposal, a second failed trial, a wrong password, a locked
    account, a captcha or a one-time code means report and stop, as today.
    Never retry in a loop.
- **`setup`** mentions the same tools as an alternative to Record sign-in for
  a new project.
- **The tool allow-list and labels** gain the two new tools: `AUTORUN_TOOLS`,
  permission names, and `src/lib/mcpTools.ts`.

### Help guide

The Record sign-in section of the help guide gets a short note on assistant
proposals and the Approve and Reject buttons. Screenshots are retaken if a
documented screen changes.

### Calls made after the builder check (mine, for the owner to check)

1. **Change-password page shape.**
   - Only the single-page shape the app already supports is handled:
     - an optional current-password field;
     - a new-password field;
     - an optional confirm field;
     - a submit button.
   - When the current-password field is absent, `fill_reset_form` skips it.
     That is the only format change.
   - A page needing more (a username, two steps, a confirmation to dismiss)
     is reported and the agent stops.
   - Detection is "two or more visible password fields", with no username
     condition.
   - A proposed `password_reset` replaces the built-in PeoplesHR reset page
     only for this environment's recipe, because recipes are per environment.
2. **Running discovery.**
   - Before inspecting, the agent ends its discovery session
     (`end_autorun_discovery`).
   - After approval it checks the sign-in, then starts a new discovery
     session at the same area. "Carry on" means a fresh session, not a resumed
     one.
   - The new tools refuse while the browser is busy, exactly as
     `check_autorun_sign_in` does.
3. **Sign-in attempts.**
   - At most **3 sign-in attempts per account** for one sign-in problem:
     inspect-with-account, trials and the follow-up check all count. The
     bridge counts them per session.
   - Trials use a different saved account than the one that failed, when one
     exists.
   - Inspecting without an account never signs in.
4. **Which commands recover.**
   - All commands that drive the live app use the same rule through the
     shared guide text: discover, gen-tests, map-menus, explore, heal, and the
     writer, explorer and healer agent rules. A mismatched sign-in page is
     equally fatal in each.
   - Map-menus' hard-stop list keeps wrong password, locked account, captcha
     and one-time code.
5. **What a proposal replaces.**
   - The assistant's draft supplies `start_url`, `steps`, `signed_in`,
     `signed_in_in_menu` and `password_reset`.
   - Everything else (after-sign-in steps, menu preferences, modes, pop-ups,
     loaders, session length) is kept from the saved recipe, as a recording
     does.
   - The notice shows only the parts the draft supplies, marking those that
     change.

## Constraints

- Rust tests only in `src-tauri/tests/suite/`. Regenerate `src/bindings.ts`
  after any `specta::Type` change, and never hand-edit it.
- No HTTP DELETE to Azure DevOps (not touched here).
- User-facing text names no URL or file path. Errors go through the existing
  sentence style.
- Passwords and typed values never cross into the assistant, the webview or
  logs. The `bindings.rs` token gate stays green.
- Theme tokens only. `ui-consistency.test.ts` is not weakened.
- One recipe per environment per project, as today.
- Changelog lines are end-user-facing.

## Left to the builder

- Exact JSON shape of the inspect output and its line limit.
- How the inspect read reuses `snapshot`/`probe` on a check browser's
  `Driver`, and how values are kept out.
- File name of the pending proposal store, and the content-hash method.
- Exact placement and layout of the proposal notice within the sign-in area,
  following the Accounts dialog's Proposed tab.
- Exact wording of the new sentences, in the existing voice.

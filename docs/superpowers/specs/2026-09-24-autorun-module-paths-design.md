# Auto Run: module paths, no direct addresses, and an account for the run

Design, agreed with the owner on 2026-09-24.

## 1. Why

An unattended run of the HR application should go the way a tester goes:

1. Open the home page (`https://hrmmainphdev01.phrsandbox.dev/hr/home/index`).
2. Sign in as the chosen account.
3. Click through the menu to the case's module.
4. Run the case's steps.

Today:

- A script signs in only when it names an account, and nothing else happens before step 1.
- A script written by an assistant usually starts with a `navigate` to a deep link, which this application sends back to its home page.
- The test case's Module field is never used by Auto Run.

Success means an unattended run of a module's cases reaches each module screen and runs its steps, with no navigation written into any script.

## 2. Owner decisions

1. A module's menu path is captured by **recording clicks** in a real browser. It is not typed as JSON, and it is not drafted by an assistant.
2. The application's menus open **on click only**, so no hover action is needed.
3. A case whose Module has no recorded path, or whose Module field is empty, is **blocked and says why**. Its steps are never run from the home page on a guess.
4. The home page is the sign-in recipe's existing `start_url`. There is no new address setting.
5. The account picked for a run applies to **scripts that name no account**. A script's own account wins.
6. Paths live in their **own file per project**, beside the sign-in recipe, not inside the recipe.

## 3. What is stored

`<autorun root>/projects/<slug>.nav.json`, next to the recipe's `<slug>.json`. It is kept on this machine only, like the recipe, and is never sent to Azure DevOps.

```json
{
  "direct_urls": false,
  "modules": [
    {
      "module": "Leave",
      "clicks": [
        { "role": "link", "name": "Leave" },
        { "role": "link", "name": "Apply Leave" }
      ],
      "arrived": "/hr/leave/apply",
      "recorded": "2026-09-24T10:00:00Z"
    }
  ]
}
```

- **`module`** is compared with the test case's Module field, trimmed and case-insensitive. Two entries that compare equal are refused on save.
- **`clicks`** is an ordered list of `Target`s: the locator type every script click already uses (role and name, text, css, chains).
- **`arrived`** is the path part of the address the recording ended on, without query or fragment. A replay of the path succeeds only if the page's address path ends up equal to it.
- **`direct_urls`** is absent in a new file, and absent means `true`. A project changes nothing until the switch is turned off.
- **A project with no nav file, or with an empty `modules` list, runs exactly as it does today (§5).**

## 4. Recording

A new **Module paths** dialog in Auto Run.

**The list:**

- It shows every recorded module and its clicks in readable form, for example `link "Leave" › link "Apply Leave"`, with where it ends.
- Each module has **Re-record**, **Try** and **Remove** (Remove asks first).
- **Record a module…** starts a new recording.
- The dialog also holds the switch "Scripts may open pages by address" (§6).

**Recording:**

1. **Choose the module and account.** The module name comes from the Module values of the cases loaded in Auto Run, or is typed. The account comes from the Accounts list.
2. **The recording browser opens.** A visible browser opens. It signs in with the recipe as that account (the same `signin::sign_in` a run uses) and ends on `start_url`.
3. **The person clicks through the menu.** The dialog lists each click as it is captured.
4. **The person chooses Stop, or Cancel.**
   - **Stop** records the page's address path as `arrived`.
   - **Cancel** closes the browser and saves nothing.

**Capturing a click:**

- **The browser side:** a capture-phase click listener, added through a CDP binding on every document, sends the recorder the clicked element. It sends the element itself and, for when the click has already navigated away, hints read on the spot: tag, role attribute, aria-label, text.
- **Building the locator:** the recorder asks Chrome's accessibility tree for the role and name of the clicked element, or of its nearest ancestor that has a role. It prefers `link`, `button`, `menuitem`, `tab` and `treeitem`, and builds `{ role, name }` from that.
- **When that doesn't work:** if the element is gone, or has no role, the locator is built from the hints (visible text).

**Checking and saving:**

- Before the path is saved, the recorder replays it in a fresh signed-in browser: sign-in, then `start_url`, then each click.
- The path is saved only if every click finds exactly one visible element and the address path ends equal to `arrived`.
- Otherwise the dialog says which click failed ("click 2, link "Apply Leave": no visible match") and offers Record again. Nothing is saved.
- **Try** runs the same check on a saved path.

**Limits:**

- One recording at a time.
- A recording and a run cannot run together.
- A recording and the supervised browser cannot be open together.

## 5. What a run does, per case

The unattended run already does one thing before step 1: sign-in, recorded as step 0. That order becomes:

1. **Choose the account.** It is the script's own account, else the account picked for the run (§7).
2. **Sign in.** This is unchanged.
3. **Go to the module**, only when the project has at least one recorded path:
   - **Go home.** If the address is not `start_url`, navigate to it. Only this runner step may do that; the "no direct addresses" rule is about scripts.
   - **Find the path.** It is found by the case's Module field. The module comes with each case the run is started with: `ReplayCase` gains `module`, filled from the loaded test case.
   - **Click the path's clicks**, with the same `runner` click a script step uses.
   - **Check** that the address path equals `arrived`.
4. **Run the script's steps.** This is unchanged.

Run review shows step 3 as its own line before step 1: `Go to Leave`, passed or failed, with a screenshot on failure.

**A case is blocked, its steps are shown as not run, and the verdict proposed is Blocked, when:**

| Condition | Sentence |
|---|---|
| The project has paths and the case's Module field is empty | `This case has no Module - set one in Azure DevOps, or record a path for it.` |
| The project has paths but none for this module | `No menu path recorded for module "X" - record one in Auto Run, Module paths.` |
| The project has paths and no account applies | `Choose an account when starting the run, or set Runs as on the script.` |
| A path click, or the arrival check, fails | `Could not reach module "X": click N, <locator> - <reason>.` |

The run moves on to the next case, as it does today after a failed sign-in.

**A `sign_in` action inside a script** lands on the home page, so the runner goes to the module again (step 3) before the next action.

## 6. Scripts may not open pages by address

The per-project switch `direct_urls` sits in the Module paths dialog as "Scripts may open pages by address". While it is off:

- **Saving is refused** for a script that contains a `navigate` action, whether absolute or relative. This applies to the Script editor, JSON import and the assistant's `save_autorun_script`. The sentence is: `this project does not allow opening pages by address: a run starts on the case's module screen - use clicks instead of "navigate" (step N).`
- **The assistant's guide** (`get_autorun_guide`) gains a section while the switch is off:
  - The run signs in and goes to the module screen before step 1.
  - The script starts there.
  - The script never navigates by address.
  - A `sign_in` action brings the run back to the module screen.
- **In a run,** a script saved before the switch was turned off fails its `navigate` step with the same sentence, and the case is blocked.
- **Not affected:** the sign-in recipe's `start_url`, the runner's own go-home step (§5), and the recorder.

## 7. An account for the run

The unattended run panel gets a **Sign in as** select, with these options:

- `Each script's own account` (the default)
- every account in the Accounts list

**How it applies:**

- **Which scripts use it:** it is used only for scripts with no `account`.
- **Saving the choice:** it is remembered per organisation and project on this machine, in localStorage.
- **An account removed since:** it falls back to the default, silently.
- **Sending it:** `auto_run_replay` gains `account: Option<String>`. The key is checked with `accounts::valid_key`, and must exist in the Accounts list, or the run does not start.

## 8. Not in scope

- Hover menus. Nothing is needed today; that would be a new action.
- Paths that depend on the account, such as different menus for different roles.
- Sub-module or feature-level paths below the Module field.
- Editing a recorded path by hand in the app. Re-record replaces it.
- An assistant tool that reads or writes paths.

## 9. Testing

**Rust integration tests in `src-tauri/tests/`:**

- **The nav file:** load and save it, duplicates refused, absent means `direct_urls` true, and Module matching (trim, case).
- **The locator from a click:**
  - built from accessibility role and name
  - the ancestor with a role preferred
  - the fallback to hints
- **The run order,** with the existing fake browser:
  - sign-in, then home, then the path, then the arrival check, then the steps
  - each blocked row in §5
  - a path click failing
  - `sign_in` mid-script going to the module again
  - no paths in the project giving exactly today's behaviour
- **The `navigate` refusal** on save (editor command, import, bridge) and in a run.
- **The run account:** the script's own account wins, a key that isn't in the list refuses the run, and none at all works as today.
- **The guide section** is present only while the switch is off.

**Frontend tests:**

- the Module paths dialog: list, Remove confirm, the switch, and the recording states driven by mocked events
- the Sign in as select, and remembering it
- **A real-browser test of record, then save, then replay** joins the ignored live browser tests (`cargo test --test browser_live -- --ignored`).

**By hand, owed by the owner:** record a real HR module path, then run a module's cases unattended.

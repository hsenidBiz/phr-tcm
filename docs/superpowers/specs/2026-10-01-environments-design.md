# Environments: one switch for website, database and accounts

Design, agreed with the owner on 2026-10-01. Builds on the per-project quirks
work (`feat/learning-quirks`), which touches the same bridge, tool and guide
files. Part B (§9) replaces one Auto Run path per module with named areas.

## 1. Why

The team tests one product (PeoplesHR) in several environments - local dev,
hosted dev, QA, automation - each used for a different purpose and each with
its own website, its own database and its own test logins. Today the app
spreads those across three unconnected settings:

- the **website** is the project's sign-in recipe start address (Auto Run's
  Site address row), one per Azure DevOps project;
- the **database** is a choice on the AI Bridge tab's Company database card
  (built-in connections: Dev read only, Dev login, QA read only, or a saved
  one), which the assistant's database tools follow;
- the **accounts** Auto Run signs in as are one list per machine.

Moving from dev to QA means changing all three by hand, and the accounts
cannot differ between environments at all. The assistant also cannot see
which test logins exist, so a person types every one.

Success means: picking "QA" in one select changes the address Auto Run and
API templates sign in to, the database the assistant queries, and the
accounts available - and a script written against hosted dev runs unchanged
against QA. The assistant can look up the test users in the active
environment's database and propose them; the person ticks the ones to keep;
and in an environment marked as a test environment the assistant can read
those logins in full.

## 2. Owner decisions

1. **An environment is a name, not an address.** Two environments may share
   the same address (local and hosted dev can both be served at one URL).
   Nothing identifies an environment by its address, keys per-environment
   data by origin, or refuses two environments with the same address.
2. **Sign-in works the same everywhere; only the address changes.** One sign-in
   recipe and one set of module paths per project; an environment supplies
   its own address and allowed sites.
3. **One active environment for the whole app.** Auto Run, API templates and
   the assistant's database tools all follow it.
4. **Logins come from the environment's database.** The assistant finds the
   test users there (read only) and proposes them; the password is the
   environment's default password, since a database holds only hashes.
5. **The assistant may read full logins in test environments.** For an
   environment marked "Test environment", the assistant may read the username
   and password of the accounts the person picked. The passwords pass through
   the AI provider; the owner accepts that for test environments only.
6. **The database write rule does not change.** Only a `_devlogin` user may
   write (`db/guard.rs`). A QA or automation login named otherwise stays
   read only.

## 3. The environment

| Field | Meaning |
|---|---|
| id | Stable, generated, never reused. What every per-environment file is keyed on. |
| name | What a person reads. Unique, case-insensitive. |
| website address | Full http(s) address, or empty: empty means "use the sign-in recipe's own address". |
| also allowed | Other origins `navigate` may go to, as the recipe's `allowed_origins`. |
| database | A built-in connection id or a saved one - the same list the Company database card offers. |
| default password | A secret, kept in Windows Credential Manager by environment id. Never in a file, never sent to the webview; the UI knows only whether one is set. |
| test environment | Off by default. On lets the assistant read this environment's full logins (decision 5). |

Environments live in one app-data file with the active environment's id;
writes are atomic.

**First run:** with no file yet, the app creates one environment, "Default",
with an empty address (so the recipe's address keeps working), the database
the person has chosen today, test environment off, and makes it active. The
existing account list becomes Default's. Nothing a person sees changes until
they add a second environment.

**Removing** an environment is refused for the active one and the last one.
Removing deletes that environment's local account and session files - local
files only.

## 4. What follows the active environment

- **Website.** One function produces the *effective* recipe: the active
  environment's address and allowed sites replace the recipe's when the
  environment has an address. Every sign-in, trip home, recording, path check
  and API template run uses it - nothing reads the recipe's address directly.
  Auto Run's Site address row edits the active environment's address (empty:
  "Using the sign-in recipe's address"). Auto Run's header reads
  `Environment <name> - <host>`.
- **Database.** Switching sets the database choice the Company database card
  already keeps, so the card, the bridge and the assistant's tools follow
  through the one existing path. Changing the database on the card changes
  the active environment's database: it is the same setting, not a second
  one.
- **Accounts.** One list per environment. An account key (`hr.supervisor`)
  names the same role everywhere, with each environment's own username and
  password - so scripts and templates, which name keys, run in any
  environment unchanged.
- **Saved sign-in sessions.** Kept per environment *and* account. A session
  saved in local dev is never restored into hosted dev, even at the same
  address - the reason decision 1 exists.
- **Records.** A run records the environment's name, and Past runs shows it.
  An API template's proof records the environment's name too; its row shows
  it, falling back to the host for proofs made before this.
- **Title bar.** With more than one environment, a pill names the active one,
  beside the Beta pill.

## 5. Logins the assistant proposes

- The assistant uses the database tools it already has (read only) on the
  active environment's database to find test users, then calls a new tool,
  `propose_accounts`, with key, label, username and optionally a role for
  each. Proposals are kept for the active environment, a new call replaces
  the previous proposals, and they are never usable on their own.
- Auto Run's Accounts dialog shows them as **Proposed by the assistant (N)**:
  a checkbox per row, the password column showing the environment's default
  password (or "no default password set - type one"), editable per row.
  **Add selected** moves the ticked ones into the environment's accounts (an
  existing key asks to replace); **Dismiss** clears them.
- A new tool, `get_accounts`, lists the active environment's accounts: key,
  label and username - and the password only when that environment is a test
  environment; otherwise it says the environment is not marked as one.
- Turning **Test environment** on shows: "The AI assistant can read the full
  logins of this environment's accounts. Use only for test environments."
- Passwords stay out of logs, run files, the activity log and events
  (`signin::redact` as today) - logs ship with bug reports.
- Both tools are gated with the Auto Run tools.

## 6. Where it lives in the UI

- **AI Bridge tab:** an **Environment** card above the Company database card:
  a select that switches the active environment at once, and **Edit
  environments**, opening a dialog to add, edit and remove them (name,
  website address, also allowed, database, default password, test
  environment). It is shown wherever the Company database card is.
- **Auto Run:** the Site address row and the Accounts dialog, as above
  (hidden with Auto Run as today).
- **Title bar:** the environment pill.

## 7. Guides

- The Auto Run guide gains a short **Environments** section: the active
  environment's name, that accounts are per environment, how to find test
  users in the database (read only) and propose them, `get_accounts`, and
  never to invent a password.
- The API templates guide says templates run against the active environment.

## 8. Out of scope

- A different sign-in recipe or module paths per environment.
- Changing the database write rule.
- Reading passwords from the database.
- Per-project active environments.
- Changelog, help-site and README changes are not in the build; the owner's
  session updates the help site and its screenshots afterwards, since the AI
  Bridge tab is documented.

## 9. Part B - areas instead of one path per module

Independent of the environments work above; built in the same plan.

**Why.** Auto Run records one way in per test-case **Module**
(`autorun/nav.rs` `ModulePath`, matched to the case's Module field). A module
such as PMS has several menu entries - Cycle Setup, Manage Cycle,
Assessments - so one path per module can reach only one of them, and cases
for the others cannot be run.

**Owner decisions.**
1. A person names an **area** and records the way into it; a module can have
   any number of areas.
2. The **script names its area** (`area`), picked by the assistant from the
   recorded areas when it writes the script and changeable in the script
   editor.
3. **Nothing that works today breaks.** Each recorded module path becomes an
   area named after its module. A script with no `area` goes to the area
   named like its test case's Module, as today.

**The area.** A recorded path plus a name: name (unique per project, case
insensitive), the Module it belongs to (one of the org's Module values, for
grouping), the clicks, the address path it arrives at, when it was recorded,
and where the recording started - everything a module path keeps today.
Saved in the same per-project file; an old file reads as areas named after
their modules. Areas are shared by every environment (they are clicks and
an address path, not an address).

**Choosing the way in for a case.**
- The script names an area: go there. A name that is not recorded refuses the
  case before it starts: `the area "<name>" is not recorded - record it in
  Auto Run, Areas`.
- No area: the area named like the case's Module, as today (today's
  sentences unchanged).

**Recording.** The Module paths dialog becomes **Areas**: grouped by module,
each area with Re-record, Try and Remove as a path has today; **Record an
area** asks for the module and an area name (the module's name is offered as
the first area's name). The Setup card row reads **Areas** with its count.

**Scripts.** `CaseScript` gains an optional `area`. Saving a script (from the
assistant, an import or the editor) refuses an area that is not recorded,
with the recorded names in the sentence. The script editor has an area
select (blank: "the case's Module"). The Auto Run guide lists the recorded
areas - name, module and the address path each lands on - and tells the
assistant to set `area` whenever the case's screen is not its module's
default area.

## 10. Testing

- Two environments with the same address are accepted, keep separate saved
  sessions, and never restore each other's session.
- First run creates Default, moves the accounts, keeps the recipe's address.
- The effective recipe: no address keeps the recipe's; an address replaces the
  address and allowed sites; and nothing signs in, navigates or runs a
  template from the recipe's own address directly.
- Accounts per environment; removing refused for the active and last
  environment.
- The default password never appears in the environments file, a command's
  output, the webview or a log.
- `get_accounts` with and without test environment; `propose_accounts`
  validation; proposals unusable until added.
- Run records and proofs written before this still load.
- The UI: switching updates the database card; the environments dialog's
  validation, removal and warning; the title-bar pill; the proposals in the
  Accounts dialog.
- Areas: an old paths file loads as areas named after their modules; two
  areas under one module both route; a script's `area` wins over its Module;
  no `area` routes by Module as today; an unrecorded area refuses the case
  and refuses a script save; the Areas dialog groups by module and records,
  re-records, tries and removes an area; the editor's area select.

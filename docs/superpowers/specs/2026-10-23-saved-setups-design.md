# Saved setups: an assistant builds a precondition once and asks for it again days later

Owner request, 2026-10-23. Approved in conversation section by section; this document is for the owner's review before a plan is written.

## Intent

An assistant often has to put a record in place before it can test anything: a leave type with its rules, an employee in a given state, a cycle at a given stage. Today it works the clicks out in an explore session, and then the clicks are forgotten. Days later, in a new session, the same or another assistant has to set up the same precondition and works it all out again.

With this feature the app keeps the actions that worked while an assistant explores. The assistant saves them under a name as a **setup**, together with a rule for recognising a suitable record that already exists. Later, an assistant asks for the setup by name. If a suitable record exists, it gets that record and nothing is created. If none exists, the app runs the saved actions with fresh values and hands back the new record.

## Owner decisions (2026-10-23)

1. Setups are asked for days later in a new session, so they are saved to disk, per project.
2. **For assistants only.** No screen, window, list or button in the app shows or runs a setup. Only assistant tools read and write them.
3. **Find first.** Asking for a setup first checks whether a suitable record already exists and uses it. A new record is built only when none is found.
4. **How "suitable" is checked:** a database check (approach A), and a browser check (approach B) as well.

## Goals

- Every action an assistant runs that works in a discovery or explore session is kept, in order, so it can be saved without typing it again.
- A setup is saved by name with its actions, its inputs and its find rule, then run by name in a later session.
- A run of a setup answers with the record to use, found or built, so the assistant can go on with its case.
- Whatever a setup builds is in the explore session's change list, so the person can read it and undo it as any explore change.

## Non-goals

- No UI. Nothing in Auto Run, the Components window, Settings or anywhere else.
- Setups are not used by scripts or runs. A case's preconditions stay checks, as today; an unattended run never builds anything.
- No replay of a whole session "as it happened". A setup is a deliberate, named save.
- No sharing of setups between projects or machines beyond the project file.

## Anti-goals (a failure even if it works)

- A setup that creates a second record when a suitable one already exists, or creates one with a name outside the test prefix.
- A setup that runs while database access is off and builds without checking because the database check could not run.
- A password, login, host, URL or query string in a setup file, an answer or a log line.
- A setup that types a username or password, or signs in.

## How it works

### 1. Keeping the actions

- While a discovery or explore session is going, each action from `discover_autorun_actions` that **succeeded** is appended, in order, to a list in that browser's discovery state, with what it ran (a `use_component` action is kept as that action, not its inner steps).
- Kept in memory only, per browser (so per slot once the several-browsers work has landed). It is kept until the next discovery starts in that browser, so the assistant can still save right after `end_autorun_discovery`.
- Not kept: failed actions, actions a save guard blocked, `try_autorun_action` rehearsals, mapping runs.
- At most 200 actions; past that the oldest are dropped and the answer to the save says the list was cut.
- A new tool, `get_kept_actions`, lists them, numbered from 1, with the area the session started in and the typed values shown as typed (never a password: sign-in actions are never kept).

### 2. Saving a setup

A new tool, `save_setup`, takes:
- `name` (unique per project; saving a taken name is a change and needs `why`), `description` (what record it builds and for what);
- `from` / `to`: the range of kept actions to save;
- `area`: the area to start from (the discovery's area by default); a run goes there from home first, as a script run does;
- `inputs`: which typed values become inputs, by action number, with a name and description each. An input marked `unique` is a value that must be new each time (a record's name): when the caller does not give one, the app makes `<test prefix><setup name>-<short stamp>`;
- `find`: the find rule (below);
- `returns`: what to hand back from a build: the new record's id (from explore's verified create) and, when an input is marked `names_the_record`, that input's value.

The save checks, and refuses with a sentence naming each problem at once:
- every action is in the kept list of this browser's current or last discovery, and every locator it uses has been seen (the same seen check as components);
- no sign-in, no username or password typed, no navigate outside the environment's allowed sites, no address built from an input;
- every value a `unique` input replaces starts with the environment's test prefix;
- the find rule is valid (below).

Saved to `projects/<project slug>-setups.json` under the Auto Run root, beside the components file and with the same write lock pattern, never in the components file. Each setup keeps: name, description, area, inputs, actions, find, returns, version, the number of changes, when it was saved, and when it last found or built a record (0 until its first run).

### 3. The find rule

Every setup has one, with one or both of:
- **`database`**: one read-only `SELECT` statement, under the same rules and limits as `db_query`'s SELECT. It may use `{prefix}` (the environment's test prefix) and nothing else as a placeholder; the app quotes it. It names the column that identifies the record (`id_column`) and optionally the one that names it (`name_column`). A row returned means found; the first row is the record.
- **`browser`**: a short list of script actions (navigate, fill, click, checks) that ends in a check. Its locators must have been seen, as in the build actions. If every action passes, the record is found; the text the last check matched is handed back as the record's name.

Which one runs:
- `database` when Database Read Access is on in the AI Bridge tab and a database is chosen;
- otherwise `browser`, if the setup has one;
- otherwise the run is refused: "This setup checks the database for an existing record, and Database Read Access is off on the AI Bridge tab. Ask the person to switch it on." It never builds without a check.

At save, a `database` rule is run once (when access is on) to prove it parses and runs; a `browser` rule must be made of kept or seen actions.

### 4. Running a setup

A new tool, `run_setup`, takes `name` and `values` for its inputs. It needs an explore session going (so whatever it builds is in the change list and can be undone); in any other discovery, or with none, it is refused with a sentence saying to start one with `start_autorun_discovery` and `explore: true`.

1. The find rule runs. If a record is found, the answer says "Found an existing record" with its id and name, and nothing else runs.
2. Otherwise the browser goes to the setup's area from home, the inputs are filled in (generated where `unique` and not given), and the actions run in order, as `discover_autorun_actions` would run them, the explore session's save rules applying.
3. The first failing action stops it: the answer names the action, what the page did, and that the setup may need saving again; what it created so far is in the change list.
4. When all pass, the new record comes from explore's verified create (and the `names_the_record` input's value), and the answer says "Built a new record". The setup's last-used time is stamped.
5. Every create it makes is recorded in the explore change list as the session's own, so undo works and an unnamed save is caught at the end as today.

`list_setups` lists the project's setups: name, description, inputs, which find rules each has, when it was last used. No values from earlier runs.

Removing a setup: `save_setup` with `remove: true` and the name. Nothing else.

### 5. Where the tools sit

- The four tools (`get_kept_actions`, `save_setup`, `list_setups`, `run_setup`) are Auto Run tools: they are offered only while Auto Run is enabled, and join the Claude Code / Cursor allow rules the app writes for Auto Run tools.
- The guide gains a short section, "Saved setups", in the explore part: look for a setup before building a precondition by hand; after building one by hand that a later case will need again, save it; always give a find rule.
- The explorer role file (`roles/explorer.md`) and `/tcm:explore` gain one line each pointing at it.
- With several browsers (the several-assistants work), kept actions are per browser, and `run_setup` runs in the caller's own browser. Two assistants running the same setup at once may both find nothing and both build; the find rule cannot prevent that, and the guide says to split work so they do not.

## Constraints

- Rust tests only in `src-tauri/tests/suite/`; source files CRLF; bindings generated.
- No HTTP DELETE to Azure DevOps; the setups never talk to Azure DevOps at all.
- No password, login, host, URL, query string or cookie in the setups file, answers or logs.
- The database rule is SELECT only, whatever the AI Bridge write switch says.
- No new cache.
- No em or en dashes in guide, role or answer text. No hidden feature named anywhere.

## Approaches considered (how "suitable" is checked)

- **A. A database check.** Exact, fast, matches how preconditions are checked today; needs Database Read Access on. Chosen.
- **B. A browser check.** Works with database access off; slower and breaks when a screen changes. Chosen as well, as the check used when database access is off.
- **C. Either, chosen per setup.** Superseded: the owner chose A and B, and the document lets a setup carry both, the database one preferred.

Storage was a builder's choice: a separate project file rather than the components file, because the components file is shown in the Components window and setups must not be.

## Order

Built after phase 2 of the several-assistants work (per-browser discovery state), so kept actions live in each browser's own state from the start. Shipped as its own beta.

## Calls made by the designer (the owner can overturn any)

- The 200-action cap, and keeping the list until the next discovery starts.
- `run_setup` only inside an explore session.
- Generated unique values as `<test prefix><setup name>-<short stamp>`.
- The database rule's only placeholder is `{prefix}`; no input values in SQL.
- A browser rule's found record is named by the text its last check matched.
- No setup runs without a find rule that can run; it never builds unchecked.

## Settled after the builder check (designer's calls, 2026-10-23)

These override the sections above where they differ.

1. **Seen check.** Explore sessions file nothing in the discovery map, by design, and that stays. A setup's locators are proven instead by the session itself: an action kept because it succeeded is its own proof, and a browser find rule's locators must appear in kept actions or be seen in the map. The area is optional: a setup with none starts from the home page.
2. **Setups are saved from explore sessions only** (a plain discovery keeps no change list, so it has no verified creates to learn from). `save_setup` outside an explore session is refused.
3. **How a run finds what it built.** At save, the setup takes the creates the assistant recorded and the app verified in the saved range (`record_explore_change`, verified by `verify_create`). For each it stores the kind, the save that made it (`METHOD /path` and its `handler`, as the change list keeps it), and where the new id sat in that save's answer. A setup with no verified create in its range is refused at save: "record what these actions create first". At run, the app watches for each save, reads the id from the same place and records the create in the change list itself, verified. Every record the build makes is recorded; the one handed back is the create the assistant marks `returns` at save (the first one by default). If a save does not answer with an id where expected, the run answers that the record was built but could not be confirmed, names the save, and asks the assistant to record it with `record_explore_change`; it is never reported as a clean build.
4. **A browser find rule's outcome.** Only its final check failing means "not found, build". Any earlier action failing means the check could not run: the run is refused, exactly as a database rule that cannot run, and nothing is built. A browser rule may use `{prefix}`, filled with the environment's test prefix at run. A browser find answers with a name and no id; that is enough.
5. **Inputs.** A typed value turned into an input is replaced everywhere it appears in the saved range (later searches, clicks and checks on the same text), stored as a template like a component's. The save answer lists each place it replaced, so the assistant can see a match by chance and refuse it.
6. **Components inside a setup.** A kept `use_component` is expanded into its steps when the setup is saved, so a setup never depends on a component the Components window can change or remove. A draft component is expanded the same way.

## Owner decision 5 (2026-10-23): a find rule only ever matches test records

A find rule can only hand back a record whose name starts with the environment's test prefix, so an assistant never takes a real person's record for test data.
- A `database` rule must name a `name_column` and use `{prefix}` in its statement; at run, a row whose name does not start with the prefix is skipped, and the first row that does is the record.
- A `browser` rule must use `{prefix}` in its final check; the text that check matched must start with the prefix, or the record counts as not found.
- A save that breaks either rule is refused: "A find rule must look only for records named with the test prefix."

## Left to the builder

- The exact stamp format, and how the app quotes `{prefix}` in SQL.
- How kept actions are stored in the discovery state, and the Rust module layout (one `autorun/setups.rs` is expected).
- Answer wording beyond the sentences quoted here, within the guide's style.

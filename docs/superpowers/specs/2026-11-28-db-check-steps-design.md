# Database check steps: design

The owner agreed the behaviour on 27 November 2026 (option C, and "That's all correct" to the shape). This document turns that into a buildable design. Short design, medium change.

## Intent

A script can check what the page shows, but not what got saved. In an HR app the screen often says "Saved" while the wrong value lands in the database, or a background job fills the value in later. A **read-only database check step** inside a saved script proves that the change landed.

## Goals

- **A new action, `expect_database`.** It holds:
  - a read-only `sql` SELECT, written by the assistant while scripting, which a person can read and edit in the script editor;
  - one expectation:
    - a row exists (`row`);
    - no row matches (`no_row`);
    - how many rows match (`count`: `equals`, `at_least` or `at_most`);
    - a column's value (`column` plus `equals` or `matches`, a regex), read from the **first row returned**.
  - an optional `timeout_ms`.
- **Finding this run's record (option C).** Values in the SQL come from:
  - placeholders for the case's setup outputs (`{{setup.x}}`, `{{fixture.id.x}}`);
  - a value captured off the page earlier with `remember_text` (only that: `remember_url` stores a whole address, rarely a usable literal, and does not feed noted values). The placeholder is new: `{{noted.<name>}}`.
  - literals the assistant writes from the case text.
  - The guide tells the assistant to prefer a captured id. When there is no id, it matches on the values the script typed, plus `ORDER BY <newest> DESC`, so leftovers from earlier runs don't fool the check.
- **Retry until right.** The step re-runs the query until the expectation holds or the timeout runs out, then fails.
  - The default timeout is `Timing.expect_ms` (10 s).
  - Per step at most 60 s. A larger value is refused on save.
- **Plain failure text.** It says what was expected and what was found:
  - `expected status 'Approved', found 'Pending'`;
  - `expected no row, found 1`;
  - `expected 2 rows, found 0`.
- **A real check.** It counts toward the expected-result floor (`is_check`). Like other checks, its literal expected values must come from the case text (`case_values`). It is not allowed inside `when_visible` / `when_hidden` / `repeat_while_visible` (`check_guarded`).

## Non-goals

- Writing to the database, or any statement other than a single SELECT.
- A structured table/column picker. It was rejected because it can't express the joins HR data needs.
- Caching query results. The query runs as written, each time.
- Checks on email, files or other stores.

## Anti-goals

- **Changing data.** The SQL goes through `db::guard` (it must classify as `Verdict::Read`) when saved, when tried and when run, and executes through `query::run_query(.., writes_on = false, ..)`. The "allow writes" switch has no effect on this step.
- **SQL injection through a captured value.** Every placeholder in `sql` is put in as a quoted literal (`setup_find::quoted_literal`, `N'..'`), never as raw text. Two cases are refused when saving:
  - a placeholder written inside quotes (`'{{noted.x}}'`);
  - a `{{noted.x}}` whose name no earlier `remember_*` in the case sets. This uses the save-time ordering check in `store.rs`.
- **A secret in a result.** A noted value read from a password field (`NOTED_SECRET`) is refused as a placeholder at run time, with a sentence. A column whose name passes `capture_redact::secret_name` never has its value shown:
  - in `detail`;
  - in the report;
  - in the step label;
  - in the video mark.
  - It shows as "the hidden value" instead. The check itself still compares the real value.
- **Leaked addresses.** No URL, server name or login appears in user-facing text. Transport errors are logged and mapped to the existing sentences.

## Behaviour details

- **Which database.** The environment's database login, resolved the way preconditions resolve it (`preconditions::environment_db`, gated by DB access `read_access`).
  - **No login, or reading off.** The step fails with one plain sentence. The run goes on to the next case exactly as it does after any failed check. Suggested sentence: "This step checks the database, but no database is chosen for this environment on the AI Bridge tab".
- **Rows.** The sqlcmd text output is parsed the way `setup_find::first_test_record` parses it:
  - header split on tab;
  - dash line and `(N rows affected)` footer skipped;
  - the capped prefix handled.
  - The existing 200-row cap applies. A `count` check on a capped answer fails with a sentence asking for a narrower query or `COUNT(*)`.
- **Column check.**
  - It requires `ORDER BY` in the SQL. A column check without one is refused on save, with a sentence asking for the newest row first.
  - The column name is matched case-insensitively. A missing column fails with "the query returned no column named X; it returned A, B, C".
- **Unreachable database.** A failure that `query::is_unreachable` / `gate::unreachable` classify as unreachable ends the retry loop early. The step fails with the network sentence. It is not flagged as a defect.
- **Logging.** Each run's SQL goes to the activity log as `db_query`'s does. Substituted values pass through the existing redaction.
- **Video and highlight.**
  - No element, so no box: add it to `has_no_element` and `changes_the_page`'s non-page-changing list.
  - It gets a video mark labelled "Check the database".
- **Report wording.** `action_words` describes the expectation, for example "Check the database: status is 'Approved'". It never prints the SQL. Secret columns use "the hidden value".

## Where it plugs in

- **Action.** Add the `Action` variant in `browser/actions.rs` with `validate` (non-empty `sql`, exactly one expectation, timeout bound). The exhaustive matches the compiler flags must all be handled.
- **Runner.** `runner.rs` carries it like `expect_response`, with a stub in `actions::run()`. It needs a database source on `InRun`, behind a small trait like `setup_find::RowSource`, so tests can fake rows.
- **Try path.** `try_autorun_action` runs it through the same source, so the assistant can prove a check while discovering.
- **Save path.**
  - Add the guard and placeholder checks to `store::check_scripts`.
  - Add an `EXAMPLES` entry (`action_help.rs`).
  - Add an arm in `case_values` and in `setup::holds_secret_placeholder`.
- **Guide** (`guide.rs`, `ACTION_KINDS`):
  - the action bullet;
  - a short "Checking the database" section on when to use it (after a save, to prove the change landed);
  - prefer a captured id, otherwise match the typed values plus newest first;
  - keep the SQL narrow.
- **Script editor.** `describeAction.ts` adds a sentence for the action. The SQL is readable and editable in the editor's existing action editing, shown in a monospace block.
- **Playwright export.** The step is not dropped, and it doesn't refuse the whole case. The export writes it as a manual check: a comment with the SQL and the expectation, plus `test.info().annotations.push({ type: "manual-check", description })`. It follows the owner's "marks it as a manual check" instead of the `no(..)` refusal other untranslatable actions use.

## Constraints

- **No DELETE anywhere** (the ADO rule is unaffected).
- **No hardcoded colours.** Bindings are regenerated, never hand-edited.
- **Tests** go in `src-tauri/tests/suite/` and `tests/`. Rust cases:
  - guard refusal on save, try and run;
  - placeholder quoting, including a captured value containing `'; DROP`;
  - a quoted placeholder refused;
  - an unknown noted name refused;
  - retry until a fake source returns the row;
  - a timeout with the expected/found text;
  - a secret column hidden in the detail and the report;
  - no login → the plain sentence;
  - unreachable → early stop;
  - capped count;
  - the export emits the manual check.

## Left to the builder

- The exact JSON shape of the expectation, for example `{"kind":"expect_database","sql":"..","column":"status","equals":"Approved"}` versus a nested `expect` object. Match the flat style of `expect_text` / `expect_count`.
- The poll interval between queries. Each query is a sqlcmd process, so something like 500 ms, not the 40 ms page poll.
- The wording of sentences, within the rules above.

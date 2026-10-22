# The assistant's scripting rules come from the Playwright repo itself

Status: draft for the owner's review, 2026-10-22 (after the builder check and the owner's answers).

## Intent

In 2.2.3-beta.20, TCM got its own rule texts for the scripting work, adapted from the Playwright repo's agent files:
- the writer, healer, case rewriter and auditor (`src-tauri/src/autorun/roles.rs`, `roles/*.md`);
- `/tcm:script-pbi`;
- the generated `.claude/agents/tcm-*.md` files.

That made two versions of the same rules. They drift apart every time the Playwright repo's files change.

The owner wants one version. TCM should read the Playwright repo's own agent files and skills and use those, so that when they change in the repo, TCM follows without anyone editing TCM.

**What the owner said:**
- Point the app at the Playwright repo's skills/agents area and use those instead of re-implementing them.
- TCM updates its guides by itself when they change in the Playwright repo.
- If the Playwright repo is not set up, the person is warned with a modal that Playwright is needed. The app ships with its current version of the agent files and skills as a fallback.
- Where a file names a Playwright tool the assistant does not have in TCM, TCM points it at TCM's own tools.
- The modal appears when the person switches to the Auto Run screen and no Playwright repo is set on the Playwright tab.
- **Where the repo's rules and TCM's disagree, TCM's win** ("Prefer TCM's healer"). For example, Playwright's healer says to fix expected values until the test passes, not to ask, and to mark a stuck test `fixme`. TCM's healer says never to change a check's value to fit the page, to report a divergence and stop, at most 3 fix cycles as guidance, and to leave cases waiting for a ruling alone. TCM's wins.
- **Rename `/tcm:script-pbi` to `/tcm:gen-tests`,** matching the repo's `/gen-test`.

## Goals

1. **Source of truth: the Playwright clone set on the Playwright tab** (the same location Export to Playwright and the Playwright tab use, `pw_repo` / `pw_export::clone`). TCM reads:
   - the agents in `.claude/agents/`: `playwright-test-planner.md`, `playwright-test-generator.md`, `playwright-test-healer.md`, `test-refactorer.md`, `spec-editor.md`, `doc-drift-auditor.md`;
   - the command `.claude/commands/gen-test.md`;
   - the repo's `CLAUDE.md`;
   - the skills in `.claude/skills/*/SKILL.md` (today `edit-spec`, `manage-test-users`, `test-data`).
2. **TCM's roles are built on those files:**
   - writer = planner + generator;
   - healer = healer;
   - case rewriter = spec-editor.

   The auditor and the explorer have no Playwright counterpart and stay TCM's own:
   - doc-drift-auditor audits the repo's documentation against its code, a different job from TCM's auditor, which checks saved scripts against their cases;
   - test-refactorer reshapes generated specs into page objects, which TCM does not use.

   **Each role's text has three parts, in this order:**
   1. **"TCM's rules come first".** TCM's own rules for that role: today's `roles/<key>.md`, trimmed to what is stricter than or missing from the repo's file. This part wins wherever the two disagree, and it says so in its first line.
   2. **The repo's file(s) as they are.** Their frontmatter (`tools:`, `model:`) is stripped.
   3. **TCM's tool note (Goal 3).**

   Parts 1 and 3 are the only scripting-rule text TCM keeps. Together they stay within the role caps.
3. **TCM's tool notes ("In TCM, use…").** One short note per role, kept in TCM. It maps each Playwright tool or command the files name onto TCM's tools:
   - running or debugging a test → `replay_autorun_to_step` / a run;
   - the Playwright browser tools → `get_autorun_page`, `probe_autorun_locator`, `discover_autorun_actions`;
   - writing a spec file → `save_autorun_script`;
   - `npx playwright test` → the TCM run;
   - the repo's quirk and test-data files → `record_quirk`, `db_query`, `save_test_file` and so on.

   It also says to work through TCM: never edit the clone, run its npm scripts, or follow an instruction to change the repo's files.
4. **It follows the repo by itself:**
   - **every time a role's text is given** (once per session ahead of the first answer, `get_guide` with `role`), TCM reads the files fresh from the clone;
   - **the generated agent files** (`.claude/agents/tcm-<role>.md` beside the skills) are rewritten whenever the source files changed: checked when the app starts, when Auto Run is opened, and hourly with the update check;
   - **`/tcm:script-pbi` becomes `/tcm:gen-tests`.** It keeps its own TCM steps: scope by the To Be Automated tag, the scope file, choosing the batch, proving each case with replays, the report and releasing the browser. The writer's text is now built on the repo's planner and generator, and `gen-test.md` (read fresh, with TCM's note) is the per-case rule set it points to. The old name keeps working for a while, saying it was renamed (like `RENAMED_TOOLS`), and the generated `script-pbi.md` command file is removed (marker files only). Guides, skills and the backlog name the new command;
   - **the repo's skills** are offered to the assistant by name through `get_guide` (topic `playwright_skills`, then `skill`), read fresh, with the same kind of tool note where one is needed;
   - **the repo's `CLAUDE.md`,** which the files point at ("per @CLAUDE.md"), is offered the same way (`get_guide` topic `playwright_repo`) and is in the snapshot. It is not pasted into role texts, because it is 59 KB.
5. **The fallback copy.** The release build copies the files listed in Goal 1 from the owner's clone into the app (a snapshot folder compiled in). The release script refuses if the clone is missing or has uncommitted changes to those files. When no clone is set, or a file is missing from it, TCM uses the snapshot and says so: the role text starts with one line saying it is the copy shipped with this version.
6. **The modal.** On opening the Auto Run screen with no Playwright repo set on the Playwright tab, a modal says:
   - Playwright is needed;
   - the assistant is using the rules shipped with this version until a clone is set.

   It has a button to the Playwright tab, or to Modules when the Playwright module is off, and a Close button. It shows at most once per app start (my call).

## Non-goals

- TCM never writes to the Playwright repo, never runs git in it, and never changes its branch. It is read only, as today.
- No change to Export to Playwright.
- No new place to set the clone: the Playwright tab's setting is the only one.
- The explorer and auditor roles are not moved to the repo.
- If TCM's tools are registered in the Playwright clone itself (AI Bridge), TCM writes its own marker-carrying `tcm-*` files there, as it does today. That is not counted as writing to the repo.

## Anti-goals

- **Two versions again.** No copy of the repo's rule wording may live in TCM's source, outside the release snapshot and the tool notes.
- **Silent staleness.** The assistant must always know whether it got the live files or the shipped copy.
- **Bloat that drowns the answer.** The repo's files are long: `test-refactorer.md` is 22 KB and `gen-test.md` 47 KB. They must not be pasted into every tool answer: a full role text goes once per session and on request, as today. The short `RULES_*` lines at the end of answers stay TCM's own, since they are about TCM's tools.

## Constraints

- **Hidden features:** never named. If the repo's files ever name one, TCM does not filter it, because they are the owner's files.
- **Text that leaves the app:** no secrets or hosts in anything TCM adds. Paths in TCM's own sentences name the file, never the full path.
- **Tests:**
  - Rust tests in `src-tauri/tests/suite/`.
  - The drift test that checks every tool name in a role text is a real TCM tool applies to the tool notes only, not to the repo's text.
  - The role caps (`ROLE_MAX_LINES` 45 / `ROLE_MAX_CHARS` 4000) apply to the notes only.
- **Generated agent files:** only `tcm-*.md` files carrying TCM's marker are ever written or removed (as today).

## Left to the builder

- How the release script snapshots the files (a folder under `src-tauri` included at build time is expected).
- The exact tool-note wording per role, after reading the repo's files.
- How "changed" is detected for regenerating the agent files: content hash against the last written.
- Where the hourly check hooks in.

## Calls I made (for the owner to check)

0. From the builder check:
   - the auditor stays TCM's own;
   - the case rewriter uses spec-editor only;
   - `/tcm:gen-tests` keeps TCM's batching and proof steps;
   - the repo's CLAUDE.md is read and shipped, and the notes tell the assistant never to edit the clone;
   - `tcm-*` files in a clone that is also the AI Bridge folder are allowed as today.

1. TCM reads the clone as it is checked out, whatever branch.
2. The tool notes live in TCM, not in the Playwright repo.
3. The modal shows once per app start, not on every visit to Auto Run.
4. The full role text still goes once per session and on request (the repo's files are long), while the end-of-answer rule lines stay TCM's.
5. The release script refuses to build when the clone is missing, or has uncommitted changes to the rule files, so the shipped copy is always a committed version.
6. The repo's skills (`edit-spec`, `manage-test-users`, `test-data`) are offered through `get_guide` by name.

# Export to Playwright: hand over what TCM knows

Date: 2026-10-17. Status: design, awaiting Avin's review.

## Intent

From the next PMS screen on, Avin writes and settles scripts in TCM Auto Run first and then
exports them to the Playwright repo (`D:\PHR-PLAYWRIGHT-AUTOMATION`), whose own pipeline
(gen-test steps 4-7: run raw, classify, heal, refactor, lint, live check) turns them into
finished specs he raises a PR for. Today the export carries the steps and drops almost
everything TCM learned while making them pass: what data a case needs and how it is built,
what it must not touch, what order it must run in, which page quirks it works around. The
Playwright side then rediscovers those by failing and healing - slowly, and sometimes
destructively (on 2026-10-16 a Definition Wizard run wiped the shared goal groups, profile
assignments and competency tree that the Performance Cycle scripts depend on).

**Goal:** the export hands over everything TCM knows, in the places the Playwright pipeline
already reads, so the pipeline needs the minimum healing and the PR is mostly as exported.

## Decisions Avin made (2026-10-17)

After the builder check: setup cases are not exported (E2); shared-draft values are
substituted (E2); cleanup is described, not exported as code (E3); only stored quirks are
exported, not validation rules (E5).

Earlier:

1. **One shared environment.** QA Automation is the only environment; no separate company for
   destructive suites. Safety has to come from a protected baseline and from not running both
   tools as the same PeoplesHR user at once (one session per user).
2. **Playwright keeps its functionality.** The export adds information; it does not override the
   repo's framework, agents, linter or conventions. The vendor healer
   (`.claude/agents/playwright-test-healer.md`) is not edited. The pipeline is `.claude/commands/gen-test.md`.
3. **Settle cases in TCM before exporting** (from now on): scripts written, run twice in a row,
   rewrite/<Invalid> JSONs imported, then export.
4. **Every test leaves the data as it found it**: own uniquely named records, cleanup that runs
   even after a failure.
5. Quirks, validation rules and similar knowledge travel through the export (as in 2).

## Non-goals

- Changing how the Playwright repo runs, heals, refactors or lints.
- A second environment, or per-tool database resets.
- Exporting TCM-only runtime machinery as code the repo must adopt (fixtures runner, run planner).
- Exporting cases whose newest run is not Passed (the existing rule stays).

## Anti-goals (a failure even if it "works")

- The healer "fixing" a test by deleting or editing a baseline record.
- An export that silently drops something that made the TCM script safe (today: `no_save`,
  setups, marks, run order - all dropped without a word; `{{setup.x}}` placeholders leak into
  specs as literal text).
- Changes that make Avin's PR diff noisier than today.

## What exists today (facts, 2026-10-17)

TCM export (`src-tauri/src/pw_export/`, `pw_repo/`):
- Writes `suites/<seg>/test-cases/<feature>.md` (case section spliced in, header
  `**User:** <key> <!-- from-tcm -->`), `suites/_generated/<kebab>.spec.ts`, and
  `suites/_generated/index.json`. Never data JSON, page objects, users.json, seed.spec.ts.
- Translated: steps, components (expanded), `when_visible`, `repeat_while_visible`, area menu
  clicks, account → repo user key. Written as comments: unchecked steps.
- Dropped silently: `no_save`, preconditions (API-flow records), setups/fixtures (the TCM save already refuses `{{setup.x}}` without a setup; the leak is only when a setup exists and the export ignores it), shared-state
  marks (`changes`/`needs_unchanged`/`needs_state`), run order, quirks, suspected-defect marks,
  recipe `close_when_shown` / `wait_while_shown`, `fail_on_unexpected_dialog`, page-error options.
- "Finish in the repo" handoff prompt (`pw_repo/handoff.rs`): one per md file, tells Claude in the
  repo to run gen-test steps 4-7, classify failures (missing precondition → record and stop),
  heal in place, capture navigation, add `**Uses:**`, tag provenance; no git.

Playwright repo slots:
- Test-case md: machine-read `## <id>` headings, `**Uses:**`, `**Navigation:** none`,
  `**Assertion floor:**`, `**User:**`; prose (read by refactorer/healer): Preconditions table,
  Test Data & Validation, Navigation Path, Test Steps, Cleanup, `**Also:**`,
  `**Added by engineer (not from ADO):**`, `**Source:**`. Re-exports already preserve `**Also:**`,
  `**Added by engineer…**` and `**Assertion floor:**`.
- `docs/app-quirks.md`: Symptom / Cause / Fix pattern / Established live / Permanent fix / Class;
  generator, healer and refactorer are told to read it first; step 4.5 appends to it.
- gen-test step 4 classifies a failure as "missing precondition" from the failure itself and
  writes it into the Preconditions table; the healer bounds block (gen-test.md) covers app
  quirks, login, missing precondition, product defect, flakes.
- Provenance values the linter accepts: `from-ticket`, `assumed`, `invented`, `derived`, `manual` (lint-tests.mjs:73); `from-tcm` is not one.
- Tags: only `@sl-only` (grepInvert). No destructive/publishes tag.

## Design: what the export adds, slot by slot

Each item says what TCM knows, where it goes, and whether the repo reads it by machine or as
prose. Everything is additive; a re-export replaces only TCM-owned content (see "Ownership").

### E1. Preconditions, filled from TCM (Preconditions table, prose)
For each case, rows built from: the script's preconditions (flow + stage, e.g. "Definition
Wizard at goal-groups stage"), `needs_unchanged` marks (as "requires: <state> unchanged"),
`needs_state` settings (as "requires: <setting> = <value>"), and the setup (as "built by TCM fixture `<id>`: <its steps as template calls with
values>"). The table has no Source column (`# | Condition`), so each TCM row ends its Condition cell with "(from TCM)" - my call. The ADO Preconditions text
stays, first.
*Why:* step 4 and the healer then see the dependency before they start, instead of inferring
it from a failure.

### E2. Setups and placeholders (spec + md) - Avin decided
- **Cases whose script has a setup are not exported.** The export refuses them with a clear
  reason ("builds its own draft with TCM fixture `<id>`; Playwright has no fixture mechanism -
  stays TCM-only"). Sync lists them as not exportable, not as Out of date. (A fixture is a list of
  API-template calls; nothing maps it to UI steps, and a `test.fixme` would stop gen-test
  refactoring the whole md file.)
- **Shared-draft values (`{{fixture.<id>.<output>}}`) are substituted** with the fixture's current
  output (e.g. the draft's id or name) at export time, and E1 adds a Preconditions row naming the
  shared draft and the fixture that rebuilds it. If the fixture has no current output, the case
  is refused. A rebuilt draft makes the spec Out of date (the substituted value is part of the
  fingerprint).
- Any other placeholder left in a step is refused, never written as literal text.

### E3. Safety facts (a new TCM-owned md block `**From TCM:**`, prose)
Not the repo's `**Added by engineer (not from ADO):**` block - that one belongs to the engineer.
One short list per case:
- `no_save`: "Look-only: must not save anything."
- Writes: which steps create/change/delete data, and on what (from the script's actions and
  marks), e.g. "step 4 deletes the goal group it created (by name)".
- Protected records: the baseline list (below) the case must never touch.
- Publishes: "publishes a cycle/definition - irreversible; run on demand only".
- Cleanup relied on: which steps put data back, also written into the md Cleanup table.
  **Described only** (Avin decided): the export writes no `afterEach`; the repo decides whether
  to add one (the goal-groups spec already does by hand).
- Suspected defect: "step N is a known product defect (TCM mark <date>): <note>" so step 4
  classifies it as a product defect instead of healing.
*Why:* this is what keeps the healer from "fixing" a test by deleting a baseline row.

### E4. Protected baseline (one repo doc, prose; plus E3 per case)
TCM keeps a project-level list of protected records (today: goal groups "Customer Focus",
"Operational Excellence"; rating methods "5-Point Scale", "Four Level Scale"; the 6-level
proficiency scale; competency groups "Core Competencies", "Technical Competencies"; the shared
drafts opened by name; published FY2026 / unpublished FY2025). The export writes/updates
`docs/tcm-protected-baseline.md` in the repo (TCM-owned file) and E3 references it.
*My call:* stored in TCM as a new project setting edited in Auto Run Setup, seeded from the
memory/notes we have; agents and `repeat_while_visible` `except` lists can read it later.

### E5. Quirks (`docs/app-quirks.md`, prose read first by generator/healer) - quirks only
Each stored TCM quirk relevant to an exported case is appended as an app-quirks entry in the
repo's own format (Symptom / Cause / Fix pattern / Established live = TCM date / Permanent fix /
Class), marked `<!-- tcm:q<id> -->` so a re-export updates its own entries and never edits the
repo's. **Validation rules are not exported** (Avin decided); they stay in TCM. Where a rule
matters for a page, record it as a TCM quirk first and it travels.

### E6. Page behaviour TCM handles itself (`docs/app-quirks.md` + optional helper note)
`close_when_shown` and `wait_while_shown` from the recipe are exported as app-quirks entries
whose "Fix pattern" names the selector and the handling, so the generator/refactorer applies the
repo's own pattern. No helper code is forced into the repo.

### E7. Run order and grouping (handoff prompt + md, prose)
From marks and the saved order: per md file, an "Order" note listing cases that must run before
others, cases that must run alone and last (publishers, those changing shared state), and
suggested `--chain` groups. The handoff prompt repeats it and tells Claude in the repo to keep
destructive and publishing cases out of routine runs. No new tag mechanism is required; a
`@destructive` / `@publishes` tag is **proposed** in the doc for the repo owner to adopt or not.

### E8. Provenance marker (md, machine-read by step 2/7)
Replace `<!-- from-tcm -->` with the repo's own marker (`from-ticket` for values that came from
the ADO case, `assumed` for values TCM chose), so step 2 account resolution and step 7
provenance tagging work unchanged.

### E9. Components → `**Uses:**` (md, machine-read)
A TCM component that maps to a repo common component (by a mapping kept in TCM, like the
account mapping) emits `**Uses:** common-components/<x>`.

### E10. The handoff prompt carries the rest
The "Finish in the repo" prompt gains, per md: the E3 safety facts summary, the E7 order note,
"read docs/tcm-protected-baseline.md and the tcm: entries in docs/app-quirks.md first", and
"a failure on a step listed as a known product defect is a product defect, not a heal".

### E11. Export warnings (TCM UI, before writing)
The export shows, per case, warnings it does not refuse on: deletes rows not named by the case
(e.g. `nth`/first-row deletes, loops without `except`), publishes, needs state another exported
case changes, quirks attached. Avin sees them before the PR.

## Ownership on re-export
TCM owns: its case section, the `**From TCM:**` block, `docs/tcm-protected-baseline.md`, and
app-quirks entries tagged `tcm:`. It never edits repo-authored blocks (`**Also:**`,
`**Added by engineer…**`, `**Assertion floor:**`, untagged quirks). This extends the existing
preservation rules in `test_case.rs`.

## Constraints
- Writes use the existing temp-file write in export.rs. Note: it is not fully all-or-nothing today (a failure part-way through the rename phase cannot undo files already moved, export.rs:984-986); the new docs/ files widen that, so they are written in the same phase and named in the failure message.
- Sync fingerprint: E1/E3/E7 content derived from marks and setups must be part of the
  fingerprint (today marks are excluded deliberately) so a changed mark shows Out of date.
- No change to which cases are exportable except: placeholder leaks become refusals (E2).
- Machine-read markers must match the repo's linter (`lint-tests.mjs`) exactly.

## Calls I made (from the builder check; Avin can overrule)
- **Repo-found precondition rows survive a re-export.** Today a re-export rewrites the
  Preconditions/Test Data/Cleanup tables (test_case.rs:77-97). From now on TCM replaces only rows
  ending "(from TCM)" and keeps every other row (e.g. ones gen-test step 4 recorded).
- **"Writes / deletes / publishes" (E3, E11) is inferred, worded "appears to"**: from button names
  (Delete, Publish, Save), `changes` marks and `no_save`. "Deletes rows not named" means an `nth`
  / first-row locator on a delete, or `repeat_while_visible` without `except`.
- **Quirks (E5):** relevant = a quirk whose source case is in the export. Its text goes in Cause;
  Permanent fix "not yet promoted"; Class "app quirk". Repo entries covering the same thing are
  left alone (no merge, no duplicate check beyond the `tcm:` tag).
- **"Run twice" (decision 3)** is a warning in E11, not a refusal; the newest-run-Passed rule stays.
- **Fingerprint (sync):** new inputs go in a new field that is hashed only when non-empty, so
  existing exports do not all flip to Out of date.
- **Provenance (E8):** the only marked value TCM writes is the `**User:**` header line; it becomes
  `assumed` (the key comes from TCM's account mapping, not the ticket).
- **`--chain` (E7):** prose suggestion only - `--chain` works only at gen-test step 1 and the
  handoff starts at step 4.
- **`**Uses:**` (E9):** when TCM writes the line, the handoff drops its "add **Uses:**" instruction
  for that case (handoff.rs:138).
- **Protected baseline (E4):** one list per project, entries `{kind, name}` (kind: goal group,
  rating method, proficiency scale, competency group, cycle), so an `except` list can use it.

## Left to the builder
- Exact md wording/format of E1 rows and the `**From TCM:**` block within the repo template.
- How the protected-baseline setting is edited in the UI.
- Component → common-component mapping storage.
- Test coverage (integration tests under `src-tauri/tests/suite/`, per CLAUDE.md).

## Backlog items (for docs/autorun/backlog-2026-10.md)
20. E1 + E2 preconditions; refuse setup cases; substitute shared-draft values (highest value).
21. E3 + E10 safety facts and suspected-defect hand-over.
22. E4 protected baseline setting + repo doc.
23. E5 + E6 quirks and page behaviour into app-quirks.md.
24. E7 run order / grouping notes; propose `@destructive` / `@publishes` tags to the repo owner.
25. E8 provenance marker, E9 `**Uses:**` mapping.
26. E11 export warnings.

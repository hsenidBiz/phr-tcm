# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository purpose

This is a **documentation-only** repository. It contains manual UI test cases for the Performance Management **Definition Wizard** (a 6-step setup flow). There is no source code, no build system, no test runner, and no git history — the deliverables are the Markdown files themselves.

Existing test suites:
- `Proficiency Profile UI - Manual Test Cases.md` — Step 4 of 6, IDs `PP-101+`
- `Goal Groups UI - Manual Test Cases.md` — Step 5 of 6, IDs `GG-101+`
- `Preview UI - Manual Test Cases.md` — Step 6 of 6, IDs `PV-101+`

Steps 1–3 (Rating Methods, Proficiency Levels, Competencies) are referenced as completed prerequisites but do not yet have their own files in this repo.

## Authoring conventions

When adding or editing test cases, mirror the existing files exactly — they are the spec.

**Scope is strictly UI-observable behaviour.** Every file opens with the line *"Database, API, and implementation details are intentionally omitted."* Do not add steps that inspect network calls, DB rows, payloads, or backend state. Validation scenarios are described in terms of the UI surface (e.g. "the page returns to the form with field-level errors after an HTTP 400") rather than the API contract itself.

**Verify in the live UI before writing.** This is a hard rule. Every field, control, label, validation message, and behaviour referenced by a test case must be observed in the live Lovable build (project `52b1498b-e151-401f-827b-960a2ea405ac`, Performance Management → Setup & Configuration → Definition Wizard) before the case is written or amended. Inherited claims from existing markdown or sample docs are not authoritative — past versions of these files referenced fields that did not exist or omitted fields that did, which had to be retro-fixed. Always drive the page in Playwright first.

**Uniform precondition** for every case:
```
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
```

**Scope is HR-Admin UI testing plus a focused security section.** The suite verifies that things work as expected for an HR Admin going through the wizard. Each step file ends with a "Security" section covering XSS payload rendering (text inputs render as escaped text), role gating (Definition Wizard nav hidden for Supervisor / Employee; direct URL access blocked), sign-out and session-timeout behaviour mid-task, and concurrent-tab handling. Input-validation cases (negative numbers, empty required fields, weightage > 100%, duplicate names, leading/trailing whitespace, very long values, large lists) and navigation cases (browser back, refresh, rapid clicks) remain in scope. Pen-test cases beyond these categories (e.g. SQL injection, CSRF, fuzzing) are still out of scope.

**Steps prefix.** Every case begins by walking the wizard from the Wizard Landing page through every prior step. The prefix is identical across all cases in a file:

- *Proficiency Profile (Step 4)* — 8-step prefix:
  ```
  1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
  2. Click "Continue to Configuration".
  3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
  4. Click "Continue".
  5. On Step 2 (Proficiency Levels), select the proficiency levels.
  6. Click "Continue".
  7. On Step 3 (Competencies), select the competencies.
  8. Click "Continue".
  ```
- *Goal Groups (Step 5)* — 10-step prefix (PP prefix + Step 4 walkthrough).
- *Preview (Step 6)* — 12-step prefix (GG prefix + Step 5 walkthrough).

Case-specific actions begin at the next number (9, 11, or 13). Renumber existing steps when adding cases or new prefix steps — do not skip numbers.

**Fixed assumption block** (top of file, after intro):
- User is logged in as an HR Admin (Sarah Thompson is the role observed in the UI).
- Wizard configuration includes whatever evaluation component the step depends on (e.g. Competencies for Proficiency Profile, Goals/KPIs for Goal Groups).
- Any other genuinely-static page state (e.g. "Position" is the default grouping type on Step 4; the Rating Methods row is expanded by default on Step 6).

Do **not** include "Steps 1–N are already completed" in the assumption block — the prefix walks them through inline.

**Wizard Landing page** is real. On a fresh entry the user lands on a "Performance Management Setup" welcome screen with an industry combobox and Goals/KPIs + Competencies checkboxes (both pre-checked). The Continue button reads "Continue to Configuration" and is disabled until an industry is picked. The Lovable preview persists state, so subsequent navigations may land directly on a saved step — when in doubt, confirm in a fresh context.

**Goal Groups Create/Edit dialog** has 5 fields, in this order: Group Name, Description, Min Goals, Max Goals, Weightage (%). Helper text under Weightage reads *"Each goal group's weightage must be less than 100%."* Buttons: Cancel + **Create** (Add) / **Update** (Edit), plus a top-right Close (×). Use the exact button labels in steps; assert the full 5-field set when describing the form.

**File structure** (in order):
1. `# {Feature} — UI Manual Test Cases` heading
2. Intro paragraph noting the step number and that DB/API/impl details are omitted; cite the relevant Phase N ADR handler spec if applicable
3. `Assumptions used for this test set:` bullet list
4. `## Topics Covered` bullet list — one bullet per major UI area, in the order they appear in the case sections below
5. Numbered `## N. Section` blocks separated by `---`, each containing the cases for that UI area
6. Final section covering footer/header buttons and any cross-cutting nav behaviour

**Test case format** (use exactly this shape — every field, in this order):
```
### {PREFIX}-{NNN} — {one-line behaviour summary}
- Preconditions: {single sentence — what state the user/page must be in}
- Steps:
    1. {imperative action}
    2. {imperative action}
- Expected Results:
    - {observable UI outcome}
    - {observable UI outcome}
- Priority: {High|Medium|Low}
```

**ID conventions:**
- Prefix is the feature initials (`PP`, `GG`, `PV`, …). Pick a new 2-letter prefix for any new feature file.
- IDs start at `101` and increment sequentially across the whole file (do **not** restart per section). When inserting a new case, append at the end of its section using the next free number — do not renumber existing IDs, since they may be referenced externally.
- Each ID appears once per file; the `—` (em dash, U+2014) separator between ID and title is intentional.

**Steps and Expected Results** use 4-space indented sub-bullets under their parent bullet. Steps are numbered (`1.`, `2.`); Expected Results are unnumbered bullets. Keep each step a single user-visible action; keep each expected result a single observable assertion.

**Priority guidance** (inferred from existing files):
- *High* — page loads, primary CTAs, save/continue happy paths, validation that blocks progression.
- *Medium* — secondary interactions, repeat/edge usages of working features, empty-state messaging.
- *Low* — used sparingly for purely cosmetic or low-impact behaviour.

**Save / Save & Exit distinction** (called out explicitly in the Preview file and applies wizard-wide): the body-panel "Save" action stays on the page; the footer "Save & Exit" action redirects to the dashboard. Preserve this distinction in expected results.

## Working on this repo

There are no commands to run — edits to `.md` files are the work product. Use Read/Edit/Write directly; there is nothing to build, lint, or execute.

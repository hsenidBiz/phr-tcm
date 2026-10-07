# Export Auto Run scripts to PHR-PLAYWRIGHT-AUTOMATION

Date: 2026-10-07. Status: design, approved in conversation; awaiting review of this
document.

## Why

PHR-PLAYWRIGHT-AUTOMATION is the team's shared Playwright project: a tester without the
application's source turns an ADO test case into a committed test that everyone runs
(`/gen-test`: fetch the case → a generator and healer produce a passing raw spec → the
repo's `test-refactorer` turns it into page objects → a 20-rule linter → commit).

TCM's Auto Run already produces the expensive middle of that pipeline - a script per
case, proven against the live application - but keeps it in its own JSON on one
machine, so none of it reaches the shared repo. This feature writes TCM's passing
scripts into a local clone of the repo at the **raw stage**, where `/gen-test`'s
generator and healer would have left them. From there the repo's own refactorer and
linter take over, and the person commits.

## Decisions (from the design conversation)

- **Raw-stage export** (not a full page-object export, not inputs-only). TCM stands in
  for the generator + healer; everything after that is the repo's own tooling, so TCM
  never has to track the repo's refactor and lint rules.
- **Into a local clone.** Settings holds the path to the person's clone. TCM writes
  files there and lists what it wrote. It never runs git, npm or Playwright in the
  clone, and never commits or pushes.
- **Navigation is reused, never written.** The repo's rule is that `src/navigation.json`
  entries are captured live with a person (`capturedBy: "user-assisted"`). TCM uses an
  existing entry for the feature; when there is none it still exports, and says the
  entry must be captured in the repo before the refactor (the refactorer stops
  without one).
- **Placement is mapped per area**, once, and remembered; the export dialog shows the
  mapping and lets the person change it for that export.
- **An export covers a PBI's passing scripts**, ticked from the Auto Run screen.

## What the repo expects (as read on 2026-10-07)

A case in the raw stage is these four things; TCM writes the first three:

1. `suites/_generated/<slug>.spec.ts` - one `test.describe('<feature title>')` holding
   one `test('<case title>')`, importing `test`/`expect` from `@playwright/test`,
   headed by:

   ```ts
   // spec: suites/<seg>/test-cases/<stem>.md
   // seed: suites/_generated/seed.spec.ts
   ```

   with `// N. <step action text>` before each step's code. Navigation at this stage is
   explicit: `page.goto('/')`, then the sidebar and module clicks. The linter skips
   `_generated/` entirely; the refactorer rewrites all of this.
2. `suites/_generated/index.json` - a flat object, `"<id>": "<slug>.spec.ts"`, 2-space
   indent, insertion order, trailing newline. Every `## <id>` heading in a test case
   must resolve through it to an existing raw spec, or the linter's `parse-error`
   fires (a rule the repo forbids relaxing).
3. `suites/<seg>/test-cases/<stem>.md` - one file per feature holding many cases, each a
   `## <id> — <title>` section (`^##\s+(\d+)\s` is what the linter reads - no other
   heading may start `## <digits>`). `<seg>` is `sl/<side>/<module>/<feature>`;
   `<stem>` is the spec stem the refactorer will produce, so it must equal the
   eventual `specs/<stem>.spec.ts`.
4. The account: a key in `src/users/users.json`
   (`<environment>.<module>.<role>[.<disc>]`), changed only through the repo's
   `npm run users -- add|set|... --apply`.

Untouched by TCM: `suites/_generated/seed.spec.ts` (rewritten per `/gen-test` run; the
`generated` project mints its login from it), `auth.setup.ts`, `src/users/users.json`,
`src/navigation.json`, page objects, data JSON and refactored specs.

## Behaviour

### Setting up

- **Settings → Auto Run → Playwright export**: the clone's folder. TCM checks it looks
  like the repo (`playwright.config.ts`, `suites/_generated/index.json`,
  `src/navigation.json`, `src/users/users.json` present) and refuses anything else
  with that sentence. The path is a per-machine setting.
- **Area mapping**: for each TCM area, `side` (admin | self), `module` and `feature`
  (kebab-case), giving `<seg>` = `sl/<side>/<module>/<feature>` and `<stem>` =
  `<feature>`. Stored with the project's other Auto Run settings. Suggested from the
  area's name the first time (Definition Wizard → `admin/performance/definition-wizard`)
  - the person confirms or edits it.
- **Account mapping**: each TCM account key → a repo user key, picked from the keys
  in the clone's `users.json`. TCM reads the KEYS only; it never reads, shows or copies
  a password from that file. Remembered per environment.

### Choosing what to export

From the Auto Run screen for the current PBI, **Export to Playwright** lists the PBI's
scripts. A case is tickable only when:

- the newest run that holds it has the person's verdict **Passed** (the refactorer's
  precondition is a raw spec that already passes), and
- every action in it has a faithful translation (below), and
- its area is mapped and its account is mapped to an existing repo key.

Every other case is listed with the one reason it cannot be exported ("latest run:
Failed", "uses upload - its file is in TCM's Test files, not the repo", "account
hr.admin has no repo user key - map it, or add one with the command below").

A missing user key shows the exact command to run in the clone, with the username
filled in and the password left as a placeholder for the person to type:
`npm run users -- add <key> --username <user> --password <password> --apply`.

### Writing

For each ticked case, in one go, refusing the whole export (nothing written) if any
file cannot be written:

- **Raw spec** `suites/_generated/<slug>.spec.ts`. `<slug>` is the kebab-case of the
  case title, shortened to 60 characters, made unique against the folder. If the index
  already maps this id, its existing file name is reused and the file replaced (a
  re-export, e.g. after a script repair).
- **Index entry**: the id added (or left pointing at the reused name); every other
  entry kept, in order.
- **Test case section** in `suites/<seg>/test-cases/<stem>.md`. A new file gets the
  repo's file header (`# Test Case Set: <Feature Title>` and its intro paragraph). The
  case's `## <id> — <title>` section is written in the repo's layout - `### Metadata`
  table (from the ADO work item), `**Side:**`, `**Navigation:**` (see below),
  `### Preconditions`, `### Test Data & Validation` (empty table - TCM has no data
  provenance to offer), `### Navigation Path`, `### Test Steps` (Step | Action |
  Expected Result, from the ADO steps), `### Cleanup / Postconditions` - and the file's
  `**User:** <key>  <!-- from-tcm -->` line. A section for an id that already exists in
  the file is replaced in place; every other section is left byte-for-byte.
- **Navigation**: if `src/navigation.json` has `entries["<module>/<feature>"]`, nothing
  is written and `**Navigation:**` says it is captured. If not, `**Navigation:**` says
  "not yet captured - capture it in this repo before refactoring", and the summary
  says so.

The summary lists every file written, each case's id → raw spec, and the next steps:
capture any missing navigation entry, run the raw spec (`--project=generated`, with the
seed set to the case's user key), ask Claude Code in the repo to run `test-refactorer`
on it, run `npm run lint:tests -- --require-specs`, commit.

### Translating a script into a raw spec

Opening: `page.goto('/')`, `expect(page).toHaveURL(/\/hr\/home\/index/)`, then the area's
recorded menu clicks, each translated like any click below. Then, per step, a
`// N. <action text>` comment and the step's actions.

| TCM | Raw spec |
|---|---|
| selector `{role,name,exact}` / `{text}` / `{css}`, `visible:false`, `nth` | `page.getByRole(...)` / `getByText` / `locator(css)`, `.nth(n)`; a chain narrows left to right; an iframe step becomes `frameLocator` |
| `click`, `fill`, `wait_for`, `drag` | `.click()`, `.fill()`, `.waitFor({ timeout })`, `.dragTo()` |
| `expect_visible/hidden/text/contains_text/count/attribute/focused` | `expect(l).toBeVisible()/toBeHidden()/toHaveText()/toContainText()/toHaveCount()/toHaveAttribute()/toBeFocused()`, keeping `timeout_ms` |
| `check_text`, `check_url` | `expect(page.locator('body')).toContainText()`, `expect(page).toHaveURL()` |
| `when_visible` | `if (await l.waitFor({ timeout }).then(() => true, () => false)) { ... }` |
| `expect_response` | `page.waitForResponse(...)` started before the step's triggering action, then status / JSON-subset `expect`s |
| `api_request` | `page.request.get(path, { params })` and the same checks |
| `reload`, `return_to_area` | `page.reload()`; the opening menu clicks again |
| `expire_session`, `press_key` | `page.context().clearCookies()`; `page.keyboard.press(...)` |
| tabs, dialogs, downloads, table-row checks | each mapped in the implementation plan from the action's own definition; any whose meaning cannot be reproduced exactly makes the case not exportable, with that reason |
| `upload`, mid-case `sign_in` | not exportable (file not in the repo; needs the repo's `actingAs`) |
| a step marked `unchecked` | its actions, plus `// Not checked: <reason>` |

Nothing is emitted that the refactorer refuses (`test.fixme`, `test.skip`). Selectors,
text and values are written as escaped TypeScript string literals, never spliced raw.

## Out of scope

- Running anything in the clone (git, npm, Playwright, Claude Code).
- Writing `users.json` or `navigation.json`.
- The refactored, page-object form; data JSON; provenance.
- `ph` / `in` countries and `common-components` placement.
- Exporting from the repo back into TCM.

## Testing

- Translator: golden tests per action kind and selector form, checked against the
  repo's raw conventions; string escaping (quotes, backticks, `${`, newlines).
- Writer, against a temporary fake clone: a new feature file; an existing feature file
  with other sections (left byte-for-byte); a re-export of the same id; index order
  kept; a refused export writes nothing.
- Eligibility: each "not exportable" reason.
- One live check by hand: export 135520's sibling set into a scratch clone, run the raw
  spec against QA Automation, run the refactorer and the linter.

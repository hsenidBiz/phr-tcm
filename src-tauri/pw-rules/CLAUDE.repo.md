# QA Automation Orchestrator

Autonomous Playwright pipeline: Azure DevOps test case -> live browser
exploration via MCP -> generated spec -> refactored into page objects. This
file governs Claude Code sessions in this repo.

## Pipeline

Given a test case ID (e.g. `#127303`) or `/gen-test 127303`:

1. **Fetch** — use the `azure-devops` MCP tool `wit_work_item` (ID as an
   integer, request `Microsoft.VSTS.TCM.Steps` explicitly). `wit_get_work_item`
   does not exist — do not reference it. Do not use search-based ADO tools
   (`search_workitem`, `testplan_list_test_cases`, etc.). Save the result to
   `suites/<seg>/test-cases/<name>.md`. **With `--describe "<intent>"`**, there
   is no ticket to fetch: step 1 instead interviews the engineer and produces
   the same artifact from their answers. Steps 2–7 run unchanged either way —
   they consume the test case, not the ticket. **With `--also "<extra>"`**,
   the ticket is still fetched and still the source of truth, and the extra
   steps it never wrote down — usually the cleanup that makes the case
   re-runnable — are drafted into its own section under an
   `**Added by engineer (not from ADO):**` marker and confirmed before
   anything is generated. It takes exactly one id and never combines with
   `--describe`. See `.claude/commands/gen-test.md` for the full interview
   and the addendum pass. **With `--sync`**, the ticket is fetched and diffed
   against the existing `## <id> —` section of its test case rather than
   written fresh; the test case is the baseline, so no revision is stored
   anywhere. The engineer classifies the drift at a confirmation gate, and
   only a behavioural change regenerates — one `test()`, never the file.
   Takes exactly one id; never combines with `--describe`, `--also`,
   `--spec`, `--chain`, `--common`, `--side` or `--user`. See
   `.claude/commands/gen-test.md` and
   `docs/superpowers/specs/2026-09-08-gen-test-sync-design.md`.
2. **Navigation capture — main session, never a subagent.** Check
   `src/navigation.json` for the `<module>/<feature>` key. On a miss: open the
   browser with the saved session, ask the user to navigate to the target page
   and say when they're there, reconstruct the side and menu chain from
   `browser_network_requests`/`browser_snapshot`, confirm the reconstructed
   chain back to the user, and only then write the entry (`side`, `menu`,
   `moduleNav` when an in-page entry point follows the sidebar chain,
   `verifiedOn`, `capturedBy: "user-assisted"`). Never delegate this to a
   subagent — it can't ask the user anything, and the sidebar is role-less
   `<div>`s an agent will loop against blind. Never write an unconfirmed
   entry. The same handoff also resolves and confirms the account the spec
   runs as — filtering `src/users/users.json` by environment and module,
   matching the ticket's Preconditions text against the surviving roles, and
   presenting a pick-list when that match isn't unique — and records it in
   the test case as `**User:** <key>`. See "Navigation, URL access, and
   reload" below and `.claude/commands/gen-test.md` for the full handoff.
3. **Generate** — do not write the Playwright code yourself. Before
   delegating, write `test.use({ userKey: '<key>' })` into
   `suites/_generated/seed.spec.ts` with the account key step 2 resolved —
   the generator has no registry access and no way to ask a human, so it
   never chooses an account itself. Then delegate to the
   `playwright-test-generator` subagent, telling it to write to
   `suites/_generated/<name>.spec.ts`. It explores the live app via the
   `playwright-test` MCP server before writing any code. **Tell it, in the
   delegation prompt, to call `generator_setup_page` with
   `project: 'generated'` and `seedFile: 'suites/_generated/seed.spec.ts'`.**
   Both arguments are required. With no `project`, the tool resolves to the
   first top-level project in `playwright.config.ts` — `unit`, whose
   `testDir` is the repo root — and writes a stray `seed.spec.ts` at the repo
   root before failing with "seed test not found". The `generated` project is
   the one whose `testMatch` finds the prepared seed.
4. **Verify raw & heal** — run
   `npx playwright test suites/_generated/<name>.spec.ts --project=generated`.
   On failure, **classify it before reacting**: if it looks like a missing
   precondition (an empty list where records were expected, a control that
   should be enabled but is disabled, a validation message naming an
   unsatisfied dependency), stop, record the precondition in the test case's
   Preconditions table, and report — do not heal. A healer pointed at a
   missing precondition will rewrite assertions until an empty page satisfies
   them. Otherwise delegate to `playwright-test-healer`. Do not hand-fix
   failing tests yourself. **Stop after 3 heal attempts** and report rather
   than looping indefinitely.
5. **Refactor** — delegate to the `test-refactorer` subagent, which turns the
   passing raw spec into the project's page-object structure. Never
   hand-refactor. Its navigation contract drops whatever navigation the raw
   spec did — the auto-fixture built from step 2's map replaces it. With
   `--spec <stem>`, one dispatch turns every ticket's raw spec into one
   combined spec; it appends if the file already exists.
6. **Verify refactored** —
   `npx playwright test suites/<seg>/specs/<name>.spec.ts --project=SL` and
   `npm run lint:tests -- --require-specs` must both pass. Report success only
   then.
7. **Run summary** — print what the pipeline invented or assumed, read from
   `provenanceOf()` (`@utils/load-data`), plus the resolved user key and its
   `from-ticket`/`assumed` marker from step 2, alongside the invented data
   values. See "Test data provenance" below.

`<seg>` = `sl/<side>/<module>/<feature>`. Country defaults to `sl` (the
baseline everything is written to today; `ph`/`in` exist only for genuinely
country-specific tests). `<side>` is `self` or `admin`. **With
`--common <component>`, `<seg>` is `common-components/<component>` instead** —
see "Common components" below.

To generate a test plan for a new area of the app instead of automating an
existing ticket, use the `playwright-test-planner` subagent.

## Non-negotiable rules

- **The `.claude/agents/*.md` prohibition applies to vendor agents only.**
  `playwright-test-generator.md`, `playwright-test-healer.md` and
  `playwright-test-planner.md` are pulled fresh via
  `npx playwright init-agents --loop claude` and must stay pristine and
  regenerable — never edit them; customise their behaviour only through the
  prompt you pass at delegation time. `.claude/agents/test-refactorer.md` and
  `.claude/agents/spec-editor.md` are **ours**, not vendor, and are fair game
  to edit whenever the pipeline's needs change. If you ever see the flat rule
  "never edit `.claude/agents/*.md`" stated elsewhere, read it as scoped to
  the three `playwright-test-*.md` files, not to these two.
- **Zero hallucination:** never guess selectors, locators, or URLs. Verify
  every interaction against the real DOM via `playwright-test` MCP tools
  before writing code.
- **Locator priority:** role-based locators (`getByRole`, `getByText`,
  `getByLabel`) over CSS/XPath.
- **Locators live in page objects**, never in specs, never in JSON.
- **Credentials live in the registry, never in test code.** Test account
  usernames and passwords are in `src/users/users.json`, committed
  deliberately — see §5 of
  `docs/superpowers/specs/2026-08-28-multi-user-registry-design.md` for the
  decision and the two consequences accepted with it. Specs and page objects
  must never hardcode a username, password or URL: a spec declares an account
  with `test.use({ userKey: '...' })` and reads `user.username`,
  `user.displayName` and the rest from the `user` fixture. `.env` holds
  `BASE_URL` and two optional knobs (`TEST_ENV`, `AUTH_TTL_MINUTES`) and
  nothing secret; it must still never be committed — if you touch
  `.gitignore`, confirm it still lists `.env`.
- **Every spec declares the account it runs as.**
  `test.use({ userKey: '<environment>.<module>.<role>' })` at the top of the
  file, recorded as a `**User:**` line in its test case. There is no shared
  login and no `setup` project; `src/fixtures/test.ts` mints or reuses that
  account's session on demand. Enforced by the `missing-user-declaration`
  linter rule.
- **An admin spec and a side-switching self spec must not share an account.**
  Which side an account is on is server-side state that persists across
  logins, so a self-side spec changes where every admin spec using that
  account starts. Give them separate registry entries.
- **No `page.waitForTimeout()` and no `networkidle`.** Rely on web-first
  assertions and auto-waiting.
- **Test data is JSON only**, **one file per feature** —
  `data/<feature>.json`, keyed by spec stem and then by case. Inputs and
  expected values live together. A single-case spec reads its slice with
  `loadData<T>(__filename)` at module scope; a multi-case spec reads each
  test's slice with `loadCase<T>(__filename, '<caseKey>')` **inside the
  `test()` body**, because `resolveUnique` mints one `{{unique}}` token per
  call and a module-scope load would give every test in the file the same one.
  No locators in JSON, ever. Data files are edited through the `test-data`
  skill (`/test-data <feature>`), never by hand and never by editing a
  workbook directly - see `.claude/skills/test-data/SKILL.md`.
  **Account identity is the one exception** — `username`, `employeeNo`,
  `displayName`, `empTitle` and `supervisor` live in `src/users/users.json`,
  not in `data/<feature>.json`, because an account is global rather than per
  feature and carries referential integrity a data file cannot express. A
  spec reads them from the `user` fixture.
- **One feature scenario set per spec file.** A spec holds one
  `test.describe` and one `test()` per ADO case it covers, with a single
  `test-cases/<stem>.md` whose `## <id> — <title>` sections record which
  tickets those are. One case per spec is still valid and is what `/gen-test`
  does when given no `--spec`.
- **`@sl-only` is the country-exclusion tag, and it is enforced, not
  decorative.** `sl/` is the baseline: the `PH` and `IN` projects run the
  whole `sl/**` tree on top of their own, and `playwright.config.ts` gives
  both `grepInvert: /@sl-only/`. So a test that is genuinely Sri
  Lanka-only — statutory EPF/ETF logic, say — is tagged at authoring time
  with `test('...', { tag: '@sl-only' })` and is then skipped by PH and IN.
  Untagged is the default and means "every country inherits this". Never tag
  a test just because it currently only runs in SL.
- **Bound the loop:** a heal is at most 3 internal cycles per dispatch and at
  most 2 dispatches total, aborting early on a repeated failure signature.
  Between the two dispatches the main session reproduces the failure live and
  hands the second healer dispatch an explicit diagnosis to act on — it
  diagnoses, it does not hand-edit the spec itself. Never retry indefinitely,
  and never refactor a spec that is still `fixme`'d. See "What is bounded, and
  what isn't" below and step 4 of the pipeline in `.claude/commands/gen-test.md`
  for the full mechanics.
- **Never `page.goto(...)` to anything but `'/'`, never that even, outside the
  three cold-start files named in the rules table below, and never
  `goBack`/`goForward` or a bare
  `page.reload()`.** This product disables direct URL navigation and browser
  back/forward for security, and `page.reload()` lands on the home page
  rather than refreshing in place. See "Navigation, URL access, and reload"
  below — this is enforced by the linter (`no-url-navigation`,
  `no-bare-reload`), not just documented here.
- **Menu chains are captured live, with a human, never guessed or
  reconstructed from a raw spec.** `src/navigation.json` is the only source of
  a verified chain; see step 2 of the pipeline and "Navigation, URL access,
  and reload" below.
- **Record provenance for every value the pipeline supplies itself.** When a
  test case doesn't specify a value the refactorer needs, invent/assume/
  derive one *and* record how in the data file's `_provenance` block. See
  "Test data provenance" below.
- **Ids at or above `900000001` are local, and mean "no ADO ticket."** They
  are allocated only by `cli/gen/local-id.mjs` (`npm run gen:next-id` prints
  the next one) — never hand-picked — and their heading carries the
  `(local — no ADO ticket)` marker, e.g.
  `## 900000001 — Delete a rating method (local — no ADO ticket)`. See
  `/gen-test --describe` in `.claude/commands/gen-test.md`.

## What is bounded, and what isn't

No single Playwright action can hang indefinitely. `playwright.config.ts` sets
`actionTimeout: 15_000`, `expect.timeout: 10_000`, `navigationTimeout: 30_000`,
and a `timeout: 90_000` per-test hard kill that caps every one of them — a
test process always terminates, including one wrapped in `.toPass()`. Several
of the app quirks in `docs/app-quirks.md` were never actually a hang; they
were a 15s action timeout misread as one.

What is *not* bounded by config is the agent loop layered on top of a test
run: the healer's own vendor instructions say "repeat until the test passes",
and the generator has no stop condition or escape hatch at all if it can't
find something. Those bounds exist only where the dispatch prompt puts
them — see step 4 of `.claude/commands/gen-test.md` for the heal loop's exact
mechanics, since the vendor agent files themselves can never carry a bound
that survives regeneration.

## Import conventions

Generated/refactored specs and page objects live several directories deep
under `suites/sl/<side>/<module>/<feature>/...`, so relative imports back to
`src/` are long and easy to get wrong. `tsconfig.json` defines path aliases
that Playwright resolves automatically — always use them instead of a
relative path:

```ts
import { test, expect } from '@fixtures/test'; // never '@playwright/test'
import { loadData } from '@utils/load-data';
import type { AppShell } from '@components/app-shell'; // only if a type is needed directly
import { EmployeeSearch } from '@common/employee-search/pages/employee-search.page';
```

`@common/*` resolves to `suites/common-components/*` — the shared page objects
a spec drives but does not own. See "Common components" below.

`@fixtures/test` exposes `test`, `expect`, and the fixtures `shell`
(`AppShell`), `dialog` (`Dialog`) and `toast` (`Toast`).

## The structural linter (`npm run lint:tests`)

`scripts/lint-tests.mjs` is built on the TypeScript compiler API — it parses
every file with `ts.createSourceFile` and inspects the AST, not regexes over
text. `npm run lint:tests` walks **two roots, `suites/` and `src/`**, skipping
`_generated/` (raw generator output is deliberately allowed to be ugly), and
enforces exactly **twenty** rules, **all hard errors — this linter has no
warning-level rule today**.

Rules split into three scopes, and the difference matters:

**Tree-wide** — every `.ts` file under either root, spec or not, because a
banned pattern hidden in a shared component is worse than one in a spec:

| Rule | What it catches |
|---|---|
| `no-evaluate` | `evaluate()`/`evaluateAll()`/`evaluateHandle()` in any file under `suites/` or `src/` — specs, page objects, components alike |
| `no-wait-for-timeout` | `waitForTimeout()` in any file under either root |
| `no-networkidle` | `networkidle` in any file under either root, including inside string/template literals |
| `duplicate-page-object` | the same exported page-object class name defined in more than one `.page.ts` under the root |
| `parse-error` | any file the TypeScript parser cannot handle, or an ADO id in a test case that `suites/_generated/index.json` cannot resolve to a raw spec |
| `no-url-navigation` | `page.goto(...)` with anything other than the literal `'/'`, anywhere; `page.goto('/')` outside `src/users/auth.ts`, `src/fixtures/test.ts` and `src/fixtures/acting-as.ts` (the three cold-start files); `goBack(...)`/`goForward(...)` anywhere |
| `no-bare-reload` | `page.reload()` outside `src/components/app-shell.ts`, which wraps it as `AppShell.goHome()` |

**Spec-scoped** — these run only on a `.spec.ts` that is inside a `specs/`
directory. `no-raw-selector` is deliberately one of them: `src/components/*`
legitimately holds raw CSS selectors (`main#phr-content`, `#phr-sidebar`) —
encapsulating them is the whole point of the component layer — so the ban is
on selectors in *specs*, not on selectors as such:

| Rule | What it catches |
|---|---|
| `no-raw-selector` | a raw CSS/XPath selector string passed to `page.click`/`fill`/`waitForSelector`/`$`/`$$`/`locator()` **in a spec under `specs/`** |
| `spec-without-test-case` | a spec with no matching `test-cases/<stem>.md` |
| `assertion-count` | a refactored spec with fewer `expect(`/`expect.soft(`/`expect.poll(` calls than its floor. The floor is the sum of the raw counterparts its test case's ADO ids resolve to — **unless** that test case records `**Assertion floor:** <count> (raw sum <count>)`, written only by `npm run spec:floor` after an approved `/edit-spec` restructure, in which case the floor is that count carried forward by however much the raw sum has moved since (`floor + (rawSumNow - rawSumThen)`), so a case appended later is still covered. A malformed floor line, or one on a spec with no resolvable raw counterpart, is itself an `assertion-count` error — never silently ignored |
| `spec-outside-specs-dir` | a `.spec.ts` that is *not* inside a `specs/` directory — which would otherwise escape every rule above and not be counted either |
| `missing-navigation-entry` | a feature directory with a spec but no `entries["<module>/<feature>"]` in `src/navigation.json` and no `**Navigation:** none` opt-out in the spec's test case (see "Navigation, URL access, and reload" below) |
| `missing-user-declaration` | a spec with no `test.use({ userKey: '...' })`, or one naming a key absent from `src/users/users.json`. Skipped entirely while that registry does not exist |
| `missing-common-import` | a spec whose test case declares `**Uses:** common-components/<component>` for a component that does not exist, or that the spec never imports from `@common/<component>`. Silent when the test case has no `**Uses:**` line — which is every test case today |

**Data-file-scoped** — these six run over every `.json` under a `data/`
directory, in their own loop, keyed off the data file rather than off any
spec. A data file is checked whether or not a spec exists for it: that is the
point, since a malformed one under the one-file-per-feature layout breaks
every spec in the feature at once, and an orphan key is *defined* by no spec
matching it. `data-shape`, `provenance-mismatch` and `orphan-case` build the
shape model from `cli/data/model.mjs` — the same model the export/sync tooling
uses — so a hand edit the round trip cannot represent is caught here, at lint
time, rather than surfacing as a broken workbook later:

| Rule | What it catches |
|---|---|
| `orphan-data` | a key in `data/<feature>.json` with no matching spec, or a data file not named after its feature |
| `data-parse-error` | a `data/<feature>.json` that is unreadable, is not valid JSON, or whose top level is not an object |
| `data-format` | a `data/<feature>.json` whose text differs from `serialize(JSON.parse(text))` (`cli/data/serialize.mjs`, the one place a data file's text is produced) - most often an all-scalar object inlined as a property value, which the serializer only ever does for an array element. Does not fire on a file `data-parse-error` already flagged |
| `data-shape` | a data file whose shape the round-trip tooling cannot represent losslessly - an array of arrays, an array mixing objects and scalars, a stem mixing loose values with case objects, an array column whose rows disagree on JSON type. Exempts `country-delta`: a country delta is a supported runtime shape (see `src/utils/load-data.ts`), just one this tooling cannot yet put in a spreadsheet - `data:excel`/`data:sync` still refuse it, but the linter does not |
| `provenance-mismatch` | a value with no `_provenance` entry, a `_provenance` entry naming a field that does not exist, or a provenance value outside the five allowed |
| `orphan-case` | a case key in a multi-case stem with no `loadCase(__filename, '<key>')` in the matching spec - `orphan-data` checks only stems |

`missing-navigation-entry` checks against the real `src/navigation.json` by
default; a caller can point it at a different (or nonexistent) file via
`lintTree(root, { navigationPath })`, which is how the linter's own unit tests
isolate it — `npm run lint:tests` itself always uses the real file.

**`parse-error` must never be downgraded to a warning or filtered out of the
output.** The parser recovers locally from most malformed input, but an
unterminated template literal or an unterminated block comment runs to EOF
and silently swallows every construct below it — no rule can fire on code the
parser never saw. `parse-error` is the only thing that makes that failure
mode visible instead of a silent pass; treat any change that would suppress
it as a break in the linter's core safety property, not a cleanup.

**A clean `npm run lint:tests` exit code is not proof by itself.** On an
empty or wrongly-scoped tree it prints `Scanned 0 spec file(s).` and still
exits 0 — deliberately, because `suites/` legitimately holds no specs yet and
the plain command must stay green. Whenever you are citing a lint run as
evidence that a spec you just wrote is clean, run
`npm run lint:tests -- --require-specs`, which turns a zero count into a
non-zero exit. Still read the `Scanned N spec file(s).` line and confirm your
own file was among those walked.

## Application facts

The canonical, machine-readable version of this list — keyed by the symptom
each behavior produces in a *failing run*, and the file both the generator
and the healer are told to read before diagnosing anything — is
`docs/app-quirks.md`. This section is the human-readable orientation to the
same territory; when the two disagree, the catalog wins, since it's the one
updated every time a heal or refactor discovers or promotes something new.

- **URL navigation is impossible.** For security, the product disables
  reaching a page by its URL, and browser back/forward are disabled too. The
  only way to a screen is clicking through the real sidebar menus. See
  "Navigation, URL access, and reload" below.
- **`page.reload()` does not log the session out, but it bounces to home.** A
  reload mid-test silently teleports the browser away from wherever it was, so
  any assertion right after it runs against the wrong page. Use
  `AppShell.goHome()` (which *is* the wrapped `page.reload()`) or
  `AppShell.revisit(...)` deliberately instead of a bare reload.
- Only new div-mounted modules are in scope. Legacy `#ifrmPage` modules
  (Employee Information) are excluded — no `frameLocator` machinery.
- Content root is `main#phr-content`; the page has two nested `<main>`
  elements, so `getByRole('main')` alone is a strict-mode violation.
- Four `[role="dialog"]` nodes are pre-rendered and hidden — always filter to
  the visible one. Use the `Dialog` component.
- Date fields pair a visible input with a hidden value input.
- Sidebar entries are role-less `<div>`s. `#phr-sidebar` starts collapsed to
  `width: 0` — never `visible`, never hoverable. `#sidebar-toggle-menu` in the
  icon rail (`#phr-navbar`, always visible) toggles it. Use
  `AppShell.expandSidebar()` / `openMenu()`; do not reimplement this.
- The two sides are entered through `/hr/home/switch?mode=ehrm` (admin) and
  `?mode=sshr` (self). `/hr/home/index` is **not** the admin entry point — it
  is the home page of whichever side the session is already on. Both shells
  look identical; the only signal telling them apart is the header's single
  `/hr/home/switch` link, which points at the *other* side. `AppShell.switchTo`
  asserts on it.
- `main#phr-content` exists only once a module is open. The home page of either
  side has just `main.phr-main-content`.
- The app has zero `data-testid` attributes.

## Retrying a genuinely flaky step

`expect(async () => { ... }).toPass({ timeout: 30000 })` is the project's one
sanctioned retry construct, and it is a scalpel, not a default. It is
permitted only when **all four** of these hold:

1. The cause is environmental or a known redundant request, not a product
   defect — the app itself invites a retry ("Please try again"), or a
   duplicate request from a double-submitting endpoint 400s while the
   underlying operation succeeded.
2. You have evidence from more than one run that the failure is intermittent.
   One red run is not evidence.
3. The wrapped block is safe to execute more than once. This is the trap:
   wrapping a *create* can leave several records behind. It is acceptable
   only when the assertion inside the block gates the retry (e.g. the retry
   re-clicks submit only because the modal is still open, so a successful
   save cannot be repeated) — reason about that explicitly and write the
   reasoning down.
4. An inline comment names the exact error message or endpoint, and the date
   it was established live.

It is **never** acceptable to use `.toPass()` to: paper over a locator that is
only sometimes right; wait out a slow page (raise the assertion's own timeout
instead); or absorb a genuine product bug. **A real defect must surface as a
failing test and be reported to a human.** Retrying it into green is how a
suite stops being worth running. Never wrap an entire test; wrap the smallest
submit-and-confirm pair, and keep the inner assertion's timeout below the
outer `toPass` timeout. The reference example is
`suites/sl/admin/performance/DW-rating-methods/specs/rating-methods.spec.ts`.
Any behavior that has earned a `.toPass()` must also have an entry in
`docs/app-quirks.md`, classed `environment flake` or `product bug`.

## Navigation, URL access, and reload

Three facts about the live application drive this whole section, and none of
them are inferable from reading the code:

1. **URL navigation is impossible.** The product disables reaching a page by
   its URL for security, and browser back/forward are disabled too. You must
   always navigate through the menus.
2. **`page.reload()` does not log you out, but it bounces you to the home
   page.** A reload mid-test silently teleports you away from where you were;
   any assertion after it runs against the wrong page.
3. **Menu chains are captured from a human once per feature, never guessed.**
   Sidebar entries are role-less `<div>`s (see below), so there is no
   role-based way to discover a chain automatically, and the product's
   URL-navigation ban means there's no way to verify a guessed one either.

**The navigation map — `src/navigation.json`.** One JSON object,
`entries["<module>/<feature>"] -> { side, menu, moduleNav?, verifiedOn,
capturedBy }`, validated informally against `src/navigation.schema.json`.
`side` is `self` or `admin`; `menu` is the ordered list of visible sidebar
labels to click, top-level first. `moduleNav` is **optional** and holds one
further in-page click *after* the sidebar chain lands on a module's home
page — a module's own entry point (e.g. "Definition Wizard") that sits
outside `#phr-sidebar` and is clicked as a link, not as a sidebar `<div>`.
Most features need only `menu`; a feature that needs `moduleNav` and is
captured without it stops one screen short, on the module home page, and
every assertion after that runs against the wrong screen. Every entry is
captured live, with a human present, during step 2 of the pipeline (see
above) or the equivalent handoff in `.claude/commands/gen-test.md` —
**never invented, never reconstructed from a raw generated spec.** The map
only ever holds features that have already been through that live handoff,
so look up the key you need in `src/navigation.json` and treat it as absent
until you have: an unmapped feature is the normal case, not an error.

**The auto-navigation fixture.** `src/fixtures/app-side.fixture.ts` exports
`navigateToSpec`, wired into `@fixtures/test` as an `auto: true` fixture (see
`src/fixtures/test.ts`). Before any refactored spec's test body runs, it
derives the spec's own `<module>/<feature>` key from its file path
(`navigationKeyForSpec` in `src/utils/navigation.ts`), looks the key up, and —
if a verified entry exists — switches side and walks the menu chain. It is a
deliberate no-op (not a throw) when no entry exists yet, so a spec mid-capture
or the framework smoke spec doesn't fail just because the map doesn't know
about it; `requireNavigation` (also in `navigation.ts`) is the loud, throwing
form for a call site that genuinely needs the chain. **Consequence for the
refactorer:** navigation is never written into a refactored spec by hand — see
the "Navigation contract" in `.claude/agents/test-refactorer.md`.

**The `**Navigation:** none` opt-out.** A spec that genuinely never navigates
to a product feature — the framework smoke spec is the only example today —
has no menu chain a human could ever capture. Rather than a silent,
path-based special case in the linter, this is opted out explicitly: a
`**Navigation:** none` line in the spec's own `test-cases/<name>.md` (see
`suites/sl/self/smoke/framework-smoke/test-cases/framework-smoke.md`), read by
the `missing-navigation-entry` rule in `scripts/lint-tests.mjs`.

**Never** `page.goto(...)` to anything but the literal `'/'`, and never that
either outside `src/users/auth.ts`, `src/fixtures/test.ts` and
`src/fixtures/acting-as.ts` — the three
one-time cold starts that load the app onto a fresh `about:blank` page before
any menu link exists to click. Never `goBack`/`goForward`. Never a bare
`page.reload()` outside `src/components/app-shell.ts`, which wraps it once as
`AppShell.goHome()` (and `AppShell.revisit(chain, side?)`, which goes home and
re-walks a chain — the right tool for a save → reload → assert persistence
check, since a bare reload would land on home and strand the assertion on the
wrong page). All of this is enforced, not just documented — see the
`no-url-navigation` and `no-bare-reload` linter rules above.

## Test data provenance

When the source test case doesn't specify a value a refactored test needs, the
`test-refactorer` supplies one anyway — and records where it came from in the
data file's `_provenance` block, alongside the value itself:

```json
{
  "add-goal": {
    "cycleName": "{{unique}} Perf Cycle",
    "category": "Personal",
    "_provenance": { "cycleName": "invented", "category": "assumed" }
  }
}
```

Values are exactly one of: `from-ticket` (the test case stated it), `invented`
(free text with no ticket basis), `assumed` (one option chosen among several
the UI offered — a dropdown, a radio group — worth flagging separately from
free-text invention because the choice can change the flow), `derived`
(computed from another value already in the data), or `manual` (a human set
this value directly, after the pipeline ran — through the `test-data` skill or
a spreadsheet).

The first four describe how the *pipeline* sourced a value it had to supply
itself. `manual` is the only one a human mints, and it is never applied
silently: it is recorded when someone deliberately overrides a `from-ticket`
value and confirms the override. See `.claude/skills/test-data/SKILL.md`.

For a test case produced by `/gen-test --describe`, `from-ticket` means "the
described test case stated it" — the described case is the source of truth
exactly as a fetched one would be. A sixth value (`described`) was considered
and rejected: it would mean changing `provenance-mismatch` in the linter, the
shape model in `cli/data/model.mjs`, and the `test-data` skill's vocabulary,
all for a labelling nicety.

`loadData<T>(__filename)` (`src/utils/load-data.ts`) strips `_provenance`
before returning, so a test never sees it; `provenanceOf(specPath)` reads it
back, which is what step 7 of the pipeline (Run summary) uses to report what
was made up.

A multi-case spec nests `_provenance` one level deeper, per case
(`_provenance: { createValid: { type: "assumed" } }`). `provenanceOf` flattens
that to `"createValid.type"`, so the run summary groups on the part before the
first dot without needing a second output format.

## Editing test data

QA engineers own the values in `data/<feature>.json` but not the format, so
every edit goes through the **`test-data` skill** (`/test-data <feature>`) —
never a hand edit of the JSON, and never a hand edit of a workbook.

**JSON is the only committed truth.** A `.xlsx` is a generated view: produced
on demand, gitignored, and rebuilt after every change so it is never stale.
The skill runs `data:excel` and `data:sync` on the engineer's behalf; those
commands are internal plumbing and print a banner saying so.

**Nothing can be corrupted quietly.** Every data cell is written as Excel text
so `"90"` cannot become `90` and `"000083"` cannot lose its zeros; the workbook
is stamped with a SHA-256 of the JSON it came from, so a sync against a file
that moved underneath it is refused; and nine gates stand between a workbook
and the JSON, any one of which writes nothing. A hand edit that dodges all of
this is still caught by `data-format`, `data-shape`, `provenance-mismatch` and
`orphan-case` in `npm run lint:tests`.

The skill is **main session only, never a subagent** - it asks the human which
case they meant and waits while they work in Excel, and a subagent can do
neither. Same rule as navigation capture.

See `docs/superpowers/specs/2026-08-28-test-data-excel-round-trip-design.md`.

## Editing a spec

`suites/**/specs/<stem>.spec.ts` is edited through the **`/edit-spec`
skill** (`/edit-spec <stem>`) — never by hand. A refactored spec is policed
by linter contracts a QA engineer has no reason to read
(`assertion-count`, `orphan-case`, `provenance-mismatch`, the chain's
closure-variable rule), so a hand edit is a fight against machinery the
editor can't see. The skill renders the spec as plain English first, applies
a confirmed change through the `spec-editor` subagent, checks the result
mechanically against a declared blast radius, and re-renders so the human
confirms the result says what they meant.

The skill is **main session only, never a subagent** — same rule and same
reason as navigation capture and `test-data`: it asks which test they meant,
whether a trade-off is acceptable, and what they observed when something
failed, and a subagent can ask nothing.

**R1 covers restructure and remove only** — reordering tests, chaining them
into `test.describe.serial` around one shared record, and deleting a test
with its data and test-case section cascaded along with it. Changing what a
test asserts or does needs live DOM verification and isn't built; adding
cleanup/teardown needs fixture machinery that doesn't exist yet either.
Adding a *new* ADO case to a spec is still `/gen-test <id> --spec <stem>`,
which appends — this skill only reshapes tests already in the tree.

**A restructure re-baselines `assertion-count`, and only through tooling.**
Chaining tests around one shared record deliberately removes the seed ceremony
downstream tests no longer need — and with it the assertions that proved that
seed. `enforceRadius` permits such a drop only on a test named under `modify`
in the beat 3 radius, and reports every one it permits (`droppedAssertions`)
so the skill can tell QA what was given up; an undeclared drop is refused
exactly as before. The linter's raw-generation sum is then the wrong floor for
that spec, so beat 4 runs
`npm run spec:floor -- <stem> --apply --reason "..."`, which measures the spec
with the linter's own counter and records
`**Assertion floor:** <count> (raw sum <count>)` in its test case. That line
is never hand-written and the number is never chosen by a human — same trust
model as a `_provenance` marker. See
`docs/superpowers/specs/2026-09-02-assertion-floor-design.md`.

`cli/spec/` mirrors `cli/data/` exactly: `model.mjs` is the **only** parser
of a spec (titles, `// N.` step comments, `loadCase` keys, `userKey`,
per-test assertion counts and a source hash), `outline.mjs` renders it as
plain English, `diff.mjs` classifies two parses and enforces the declared
blast radius (`enforceRadius`), `show.mjs` is the `npm run spec:show` entry
point, and `floor.mjs` is the `npm run spec:floor` entry point that measures
and records a re-baselined assertion floor.

The radius covers more than a rewritten body. A test's `.serial` wrapping, its
`describe` membership and its position all change how it runs while leaving
its source hash untouched, so each is refused unless the test is named under
`modify` — chaining three tests into `test.describe.serial` means declaring
all three. The spec's `userKey` is the one thing **no** declaration can
authorise: changing the account a spec runs as is outside R1's scope and is a
deliberate human edit, never a side effect of a restructure.

See `docs/superpowers/specs/2026-08-30-edit-spec-skill-design.md`.

## Common components

Some components are loaded by many modules and owned by none — an employee
search with its own filtrations, say. They live in a top-level tree of their
own, because they belong to no country, no side and no module:

```
suites/common-components/<component>/
  pages/<component>.page.ts     the shared page object, imported via @common/*
  specs/<component>.spec.ts     the component's own tests
  test-cases/<component>.md     its own ADO ids, or a local id when it has none
  data/<component>.json         only if its own tests need data
```

Two directories above `specs/`, not four. **No `<country>`**, because SL, PH
and IN all run this tree — `playwright.config.ts` puts
`common-components/**` in all three projects' `testMatch`. **No `<side>`**,
because which side reaches a component, and any in-page `moduleNav` click
after the sidebar chain, is recorded in `src/navigation.json` under
`common-components/<component>` beside the menu chain, where every other
navigation fact already lives.

**Never tag a test in this tree `@sl-only`.** PH and IN both `grepInvert` that
tag, so tagging one would skip a component those countries genuinely share, in
the two projects that most need it covered.

**Build the component first; the dependent case reuses it.** This ordering is
the whole design, not a convention:

```
/gen-test <its own ADO id> --common <component>      # it has a ticket
/gen-test --describe "..." --common <component>      # it has none
```

`--common <component>` is purely a destination flag — it replaces `<seg>` and
changes nothing else. A common component is **not** restricted to
`--describe`: fetch a real ticket whenever one exists, exactly as anywhere
else in this repo.

**A case that depends on a component declares it** in its test case, one line
per component, beside `**User:**` and `**Navigation:**`:

```markdown
**Uses:** common-components/employee-search
```

Step 2 of the pipeline asks a human whether the case touches a shared
component, and **stops before generating** if a named one is not built yet.
That gate is in the live handoff rather than in the linter because the reverse
direction — a spec that drives a component without declaring it — is not
mechanically detectable. `missing-common-import` is the backstop: it fails a
spec whose test case declares a component the spec never imports.

**Who knows what.** The generator is told nothing about this tree, on purpose.
It explores live and writes raw interactions for the shared component like
anything else, because the raw spec must run and pass at step 4 and is the
`assertion-count` baseline — steering it away would lower that bar, and handing
it a stored page object would invite trusting locators over the live DOM. The
swap happens entirely in the **refactorer**, which is told the component's
public API at dispatch time so it can recognise which part of the raw spec is
the component.

**Assertions stay in the spec.** `assertion-count` counts `expect(` in the
spec file only, so an assertion moved into a common page object disappears
from that count and fails the rule. A common page object exposes locators and
actions; the spec asserts on them.

Nothing is built in this tree yet — see `suites/common-components/README.md`.

## Directory conventions

```
suites/sl/<side>/<module>/<feature>/
  test-cases/<name>.md     ADO-derived markdown, source of truth for <name>
  specs/<name>.spec.ts     one feature scenario set per file, refactored
  data/<feature>.json      inputs + expected values for every spec in the feature,
                           keyed by spec stem then by case; only if any test needs data
  pages/<feature>.page.ts  locators, one per feature screen, shared across its tests
suites/common-components/<component>/
                           cross-cutting components owned by no module (see above).
                           Same four subdirectories; two levels above specs/, not four.
                           Run by SL, PH and IN alike; imported via @common/*
suites/_generated/         raw generator output, exempt from the linter, never hand-edited
suites/_generated/index.json
                           "<ado-id>": "<slug>.spec.ts" - how the linter resolves a
                           test case's ADO ids to the raw specs it must count
suites/sl/self/smoke/framework-smoke/
                           framework self-test, hand-written, no ADO ticket, opted out of
                           missing-navigation-entry ("**Navigation:** none" in its test
                           case). The only runtime proof that the @fixtures/@utils aliases
                           resolve, that loadData finds its JSON at real depth, that
                           storageState carries a session into a test, and that
                           AppShell.switchTo + expandSidebar work. Keep it green.
src/users/users.json       the account registry — credentials + identity, keyed
                           "<environment>.<module>.<role>[.<discriminator>]"
src/users/registry.ts      resolveUser(key) — parses the key, applies TEST_ENV,
                           resolves supervisor one level
src/users/auth.ts          ensureAuthFile(user) — lazy UI login, cached per account
                           under .auth/<environment>/
src/fixtures/acting-as.ts  actingAs(key) — a second signed-in context inside one
                           test, for submit-then-approve flows
src/navigation.json        the navigation map — verified {side, menu} chains, keyed
                           "<module>/<feature>", captured live only (see above)
src/components/            cross-cutting primitives (AppShell, Dialog, Toast, ...)
src/fixtures/test.ts       extended `test` injecting page objects
src/fixtures/app-side.fixture.ts
                           auto fixture: walks a spec's navigation.json chain before its
                           test body runs (see "Navigation, URL access, and reload")
src/utils/load-data.ts     loadData<T>(__filename) — derives data path from spec path,
                           strips _provenance; provenanceOf(specPath) reads it back
src/utils/navigation.ts    navigationKeyForSpec, lookupNavigation, requireNavigation
cli/data/                  test-data tooling: model.mjs (JSON <-> shape model, the only
                           reader/writer of a data file), render.mjs, mutate.mjs,
                           diff.mjs (the nine sync gates), xlsx.mjs (the only file
                           importing exceljs), edit/export/sync CLI entry points
cli/state/run-options.mjs  the run options a human sets - null means "emit no
                           flag"; ROWS/describe/validate are the single
                           definition of what an option is
cli/state/options-file.mjs loads and validates qa-options.json (gitignored,
                           per-developer) in place of the old F12 screen -
                           see "npm run qa console runner" below
cli/screen-prompt.mjs      an ask/log pair that renders inside a screen, so
                           cli/users/flows.mjs never needs to change
cli/text-mode.mjs          a scrollable read-only panel (the run-list preview)
cli/render/field.mjs       a pure one-line text input: model + renderer
.claude/skills/test-data/  the only sanctioned way to edit a data file - see below
cli/spec/                  spec-editing tooling, mirrors cli/data/: model.mjs (AST ->
                           shape model, the only parser of a spec), outline.mjs (the
                           plain-English render), diff.mjs (enforceRadius, the blast-radius
                           gate), show.mjs, the npm run spec:show entry point, and
                           floor.mjs, the npm run spec:floor entry point that measures
                           and records an assertion floor after an approved restructure,
                           splice.mjs (verifySplice - proves a sync changed exactly
                           one test) and locate.mjs (ADO id -> its three artifacts,
                           the npm run spec:locate entry point)
.claude/skills/edit-spec/  the only sanctioned way to edit a refactored spec - see above
docs/                      architecture & setup docs
```

## Commands

| Command | Purpose |
|---|---|
| `npm run qa` | browse the suite and run a selection (see below) |
| `npm run qa:options` | show `qa-options.json`'s effective values and any errors; `-- --init` writes a template |
| `npx playwright test --project=SL` | run the Sri Lanka suite |
| `npx playwright test --project=PH` \| `--project=IN` | run a country suite (SL baseline minus `@sl-only`) |
| `npx playwright test --project=unit` | run framework unit tests |
| `npm run lint:tests` | structural guardrail (see above) |
| `npm run data:show -- <feature>` | show a feature's test data as a grid (the skill runs this) |
| `npm run data:edit -- ...` | change a value (the skill runs this; `--apply` to write) |
| `npm run data:excel -- <feature>` | generate the feature's workbook (the skill runs this) |
| `npm run data:sync -- <feature>` | pull a workbook's edits back into JSON (the skill runs this) |
| `npm run spec:show -- <stem>` | show a spec as a plain-English outline (the `edit-spec` skill runs this) |
| `npm run spec:floor -- <stem>` | show a spec's assertion count and its raw-counterpart sum; `--apply --reason "..."` records the floor (the `edit-spec` skill runs this) |
| `npm run spec:locate -- <ado-id>` | resolve an ADO id to its spec, test case, test title and raw counterpart (the `--sync` flow runs this) |

## The `npm run qa` console runner

For QA engineers who should not have to compose a `npx playwright test`
invocation by hand. **One command, no flags** - everything is a keypress
inside the console. It is a **launcher, not a runner**: it helps a human build
a `--test-list` file, verifies it, then hands the terminal to Playwright, so
what they see is Playwright's own reporter output.

Source lives in `cli/`, plain `.mjs`, unit-tested by the `unit` project. Two
dependencies are used under `cli/`: `exceljs`, by `cli/data/xlsx.mjs` alone,
and `typescript`, by exactly three modules — `cli/users/references.mjs`,
`cli/spec/model.mjs`, and `cli/spec/floor.mjs` (transitively, through
`scripts/lint-tests.mjs`) — all of which read a spec with the compiler API
rather than with regexes, so the CLI and `scripts/lint-tests.mjs` agree on
what they read out of a spec. Everything else — the test-data model, renderer
and gates, and the whole console — is deliberately dependency-free so it is
testable without either. `cli/qa.mjs` therefore imports `cli/users/users.mjs`
**dynamically**, inside the users screen: loading `typescript` costs ~400 ms,
and a run that never opens that screen must not pay it. The other two
importers cost the console nothing, because **nothing under `cli/` imports
`cli/spec/` at all** — it is reached only through `npm run spec:show` and
`npm run spec:floor`. Keep it that way, or `cli/spec/` needs the same
dynamic-import treatment.

- **The tree opens folded.** `→` opens a row, `←` closes it, `F7`/`+` opens
  everything, `F8`/`-` closes it. The suite is going to grow; opening on every
  test in the repo would not scale.
- **All countries in one tree.** `sl`, `ph` and `in` are top-level folders, not
  a mode to switch. Each spec runs under the project that owns its own
  directory (`sl/**` under SL, `ph/**` under PH, `in/**` under IN), and
  `--project` is repeatable, so a selection spanning countries runs in one go.
  This also removes the duplication: PH and IN both run the whole `sl/**` tree,
  so an unfiltered listing reports every SL spec three times -
  `filterToOwningProject` keeps the one whose project matches its path.
- `space` ticks a row, `F3`/`r` runs what is ticked, `F2`/`a` runs everything,
  `F4`/`b` cycles the browser between hidden, shown and debug, `F9`/`f` re-runs
  failures, `F10`/`t` picks a report from the run history, `F11`/`d` previews
  the command, `F6`/`u` opens the account screen. **Every F-key also has a
  letter**, because VS Code swallows F1 and F5 and MinTTY may deliver no F-keys
  at all.
- **The account screen (`F6`/`u`) manages accounts, not just lists them.** Its
  own key table, `USERS_BINDINGS` in `cli/input/keymap.mjs`, is deliberately
  separate from the tree's `BINDINGS` — `a`/`d`/`n` are already taken there
  (run all, preview, clear) — so read that table, not this one, before
  documenting a letter. Inside the screen: `a` adds an account, `e` edits one
  field of one, `d` deletes, `n` renames, `p` toggles revealing passwords
  (masked by default), `q`/`Esc`
  returns to the tree, and movement is the usual arrows/j/k/PageUp/PageDown/
  Home/End. `e` prompts for a field by number and then its value, and is the
  only way to change a `username`, `password` or identity field on an account
  that already exists — `rename` moves the *key*, never a field. Changing a
  username or password also deletes that account's cached `.auth` session,
  because the cache is keyed by key shape and trusted on mtime for eight
  hours, so leaving it would keep every run signed in with the old
  credentials; the four identity fields cannot affect a login and leave it
  alone. Delete confirms with `y`/`n` and **refuses outright** — no prompt
  at all — while any spec's `test.use`, an `actingAs` call, a test case's
  `**User:**` line, or another account's `supervisor` pointer still names the
  key. The CLI's `npm run users -- rename <old> <new> --cascade` (`n` in the
  console, which always plans with the cascade) rewrites those same three
  call-site forms plus the `supervisor` pointer instead of refusing on them —
  the pointer is repointed inside the registry object, not by a text edit —
  and folds the edit list into the `y`/`n` confirmation; declining writes
  nothing. `--cascade` never rewrites a *mention* — the CLI's own help text,
  a skill's example command, a test fixture in `cli/users/*.test.mjs` that
  merely contains the key's text — those are reported by file and line for a
  human to review instead, since rewriting free text blind would corrupt a
  fixture that is supposed to contain the string. That report comes back on
  the plan, so both surfaces print it, and it covers the stale text left
  *inside* a rewritten file as well as whole files nothing rewrote — but
  never a sibling checkout under `.claude/worktrees/`, which is another
  branch's copy of the same repo. The provenance marker on a
  rewritten `**User:**` line (`<!-- assumed -->`/`<!-- from-ticket -->`) is
  left alone either way: it records how the account was *chosen*, and a
  rename changes only how the key is spelled. Rename — like `rm` — also
  deletes the OLD key's cached `.auth` session, which is named by key shape
  and would otherwise be inherited by the next account minted at that key.
  An account with no spec reference reads
  `unused` only when *nothing* names it; one named solely by a test case's
  `**User:**` line or another entry's `supervisor` reads `no specs`, because
  `unused` is the screen's marker for "safe to delete" and `d` refuses those.
  The console and `npm run users` share one rulebook — both
  call `cli/users/actions.mjs` — so a refusal reads identically on either
  surface. `npm run users` itself is not deprecated — it remains the only
  route for `import` (which takes a file path and replaces the whole registry,
  not a per-keystroke action this screen should offer), `check`,
  `list --json` (which `.claude/commands/gen-test.md` calls during test
  generation), `add --supervisor` (the console's `add` cannot set a supervisor
  pointer at all — though `e` can set one on an account that already exists),
  and `rm --dry-run` (the console's `d` plans and refuses in
  one step, so there is no reference listing short of attempting the delete).
  Everything else the CLI mutates has a console equivalent, `set` included:
  `npm run users -- set <key> <field> <value> --apply` is `e`.
- **Run options live in a gitignored JSON file, `qa-options.json`, at the
  repo root — not on a screen.** A full-screen text field (the old `F12`/`o`
  options screen) rendered correctly but a real terminal (Windows Terminal,
  156x41) sometimes painted it corrupted - duplicated rows, a cursor marker on
  two rows, the title scrolled away - a rendering fault a byte-level trace
  could not attribute to this console's output stream, so the screen was
  removed rather than chased blind. The options themselves are unchanged:
  `retries`, `max failures`, `timeout`, `global timeout` and `trace` map to
  the Playwright flags of the same names, and **`null` (or the key being
  absent) means "emit no flag", never "emit the documented default"** -
  `playwright.config.ts` sets `timeout: 90_000` while Playwright's own
  `--help` calls 30000 the default, so passing the documented default would
  cut this project's per-test budget by sixty seconds while claiming to
  change nothing. `npm run qa:options` shows every option's effective value
  and whether it came from the file or is a default, the file's path and
  whether it exists, and any validation error; `npm run qa:options -- --init`
  writes a template with every key `null`, refusing to overwrite one that
  already exists. Every key is validated through `cli/state/run-options.mjs`'s
  own `validate()` - an unknown key, or a value it rejects, falls back to that
  key's default and is reported rather than failing the whole file, and the
  file being absent (the common case) or empty is not an error at all. **The
  console must never crash or refuse to start over a bad file** - `cli/qa.mjs`
  loads it once at startup, before anything takes the alternate screen (the
  one place a bare-terminal message is still correct), prints any errors
  there, and carries on with defaults for whatever could not be read. Because
  it is a file rather than session state, a set option now persists across
  relaunches until the file is edited or deleted - unlike the old screen,
  where nothing was written to disk and every relaunch started at defaults;
  the header (`renderStatus`) still lists every non-default value for the
  same reason it always did, so a set option is never invisible. `--debug` is
  reached through the browser cycle rather than as an option, because
  Playwright documents it as shorthand that forces `--timeout=0
  --max-failures=1 --headed --workers=1` - `buildRunArgs` therefore
  suppresses those four rather than emitting values the run would ignore. A
  debug run of more than one test confirms first, on both the
  ticked-selection path and the `F9` re-run-failures path. `--video` and
  `--screenshot` are deliberately absent: neither is a CLI flag, both are
  config-only, and `--trace` covers the need without touching
  `playwright.config.ts`.
- **Every prompt renders inside a screen.** The account flows, the run-list
  preview and the console's empty-state messages never leave the alternate
  screen. `cli/users/flows.mjs` is unchanged and unchangeable for this
  purpose - it is handed a different `ask`/`log` (`cli/screen-prompt.mjs`),
  which is what keeps the console and `npm run users` refusing identically.
  There are two deliberate exceptions, both raw-terminal interactions this
  rule does not cover: `pause()` after a real Playwright run, whose output
  must not be painted over, and `confirmDebugRun` (`cli/qa.mjs`), a second
  bare-terminal confirmation asked before a multi-test `--debug` run - added
  by the very change whose goal was to remove these, and not yet moved into
  a screen.
- Non-interactive terminals fall back to a numbered prompt automatically.
  `--dry-run` exists for scripting and tests only.

**The one rule that must never be relaxed:** every run is dry-counted with
`--list --test-list` first, and refused unless the match count equals the
selection count. Playwright's "No tests found" guard explicitly exempts
`--test-list` runs (`runner/index.js:6003`), so a list that matches nothing
**exits 0 having run no tests**. For this audience a green run that did nothing
is the worst possible failure. See `cli/run/verify-count.mjs`.

Two more facts that are easy to get wrong when touching `cli/`:

- The `--test-list` **title** path is prefix-matched but the **file** path is an
  exact string compare. A whole feature is one line *per spec file*; a line
  naming a directory silently matches nothing.
- A file-level suite's `title` in `--reporter=json` uses host separators
  (backslashes on Windows); only `spec.file` is POSIX. Reading the wrong one
  produces a run list that matches nothing.

See `docs/superpowers/specs/2026-08-25-qa-framework-rebuild-design.md` for the
full design.

## Report history

**Playwright keeps exactly one report unless you make it keep more.** The HTML
reporter rewrites its output folder every run, and the output directory is
emptied at the *start* of a run rather than per test - so an unrelated run
deletes the traces you were about to read.

`playwright.config.ts` therefore gives every run its own folder,
`reports/<runId>/`, plus a sibling `reports/<runId>.json` for the navigator's
stats. `runId` is a **local** timestamp (`2026-08-28T13-42-05`) - dashes
because a colon is not a legal Windows path character.

- **Nothing is ever pruned.** Whoever cloned the repo decides what to delete.
  `/reports/` is gitignored.
- **Every entry point archives**, because this lives in the config rather than
  in `cli/`: `npm run qa`, a bare `npx playwright test`, and pipeline steps 4
  and 6. This is what stops step 6 (`--project=SL`) destroying step 4's raw
  traces (`--project=generated`).
- **`npx playwright show-report` needs the folder named now**
  (`npx playwright show-report reports/<runId>`). There is no fixed
  `playwright-report/` any more. `F10` in `npm run qa` lists the runs instead.
- **An archived report is self-contained** - path attachments are copied into
  it - so it still opens correctly after `test-results/` is wiped.
- **The summary JSON is a sibling of the report folder, never inside it.** The
  HTML reporter removes its own output folder before writing, so anything
  placed in there survives only by reporter ordering.
- **Reports are not per feature, and must not become per feature.** Playwright
  writes one report per invocation, so per-feature folders would force one
  invocation per feature - which breaks `F9`, since `.last-run.json` is
  rewritten by each invocation and would remember only the last feature's
  failures. The report already groups by spec file, `describe` block and test
  title.

See `docs/superpowers/specs/2026-08-28-report-history-design.md`.

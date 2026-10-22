---
description: Run the full ADO ticket -> Playwright test pipeline (fetch, generate, refactor)
argument-hint: <ADO test case ID...> [--describe "<intent>"] [--also "<extra>"] [--spec <stem>] [--chain] [--side self|admin] [--user <key>] [--common <component>] [--sync]
---

Run the pipeline for Azure DevOps Test Case `$ARGUMENTS`, per @CLAUDE.md.

## Arguments

- **One or more ADO test case IDs.** Every ID in one invocation must resolve to
  the same `<seg>`. If they do not, **stop and report** rather than scattering
  specs across features — the engineer meant to run two invocations.
- **`--describe "<intent>"`** — replaces the ADO id as the source of truth: the
  pipeline interviews the engineer and writes the test case from their answers
  instead of fetching a ticket. Mutually exclusive with an ADO id — **if both
  are given, stop and report**, because the two disagree about what the source
  of truth is, and guessing which the engineer meant is how a test ends up
  documenting the wrong thing. `--spec <stem>` is the common pairing (appending
  a ticketless test to an existing spec); without it the spec is named from a
  slug of the description. To *add* to a ticket rather than replace it, use
  `--also`.
- **`--also "<extra>"`** — steps the case needs that the ticket never wrote
  down. The ticket stays the source of truth; this is an addendum to it. Most
  often cleanup that makes the case re-runnable: applying a competency
  template only works once, so a ticket that stops at "apply" leaves a test
  that passes exactly one time.

  ```
  /gen-test 135270 --also "remove the added rating method" --spec competencies
  ```

  - **Requires an ADO id.** `--also` with `--describe` is a **stop and
    report**: with no ticket there is nothing to augment, and the description
    is already the whole statement of intent — the extra steps belong inside
    it.
  - **Requires exactly one id.** `/gen-test 1 2 --also "..."` stops and
    reports rather than guessing which ticket the extra steps belong to.
    Attaching a cleanup step to the wrong ticket's raw spec is a silent
    wrong answer, and the engineer meant two invocations.
  - The addendum is **drafted into steps and confirmed, never interviewed** —
    see step 1 below.

  **`--also` cleanup is test steps, not teardown.** There is no `afterEach`
  fixture machinery in this repo (the `edit-spec` skill's R1 scope notes the
  same gap), so a test that fails before reaching the cleanup step still
  leaves its record behind and the next run hits the original problem. Say so
  when it matters; do not invent a fixture to close the gap.
- **`--spec <stem>`** — put every case in this invocation into
  `suites/<seg>/specs/<stem>.spec.ts`. Creates it if absent, **appends** to it
  if present; these are the same operation, so there is no separate "add to
  existing" flag. Without `--spec`, each ID gets its own spec named from its
  ticket, which is the existing one-case-per-spec behaviour and stays the
  default.
- **`--chain`** — only meaningful with `--spec`. Makes the combined spec
  `test.describe.serial`, with the record created in the first test shared by
  the rest. Use it only when the ADO cases genuinely depend on one another.
  **Never infer a chain** from tickets that merely look related; if the
  engineer did not pass `--chain`, the tests are independent.
- **`--sync`** — re-align an already-generated case to its ticket after the
  ticket changed. The ADO id is fetched again and diffed against the stored
  `## <id> —` section in its test case, which **is** the baseline: no revision
  stamp is kept anywhere, and none is needed, because a human initiates every
  sync. See step 1S below for the flow and
  `docs/superpowers/specs/2026-09-08-gen-test-sync-design.md` for why.

  ```
  /gen-test 135171 --sync
  ```

  - **Requires exactly one ADO id.** Two ids is two invocations, exactly as
    `--also` already rules — a delta belongs to one ticket.
  - **`--describe` is a stop and report.** `--describe` means there is no
    ticket; there is nothing to sync against.
  - **`--spec`, `--chain`, `--common` are a stop and report.** These declare
    where a *new* artifact goes. Under `--sync` the destination is discovered
    with `npm run spec:locate -- <id>`, never declared. An engineer passing
    one believes they are creating something, and that belief needs
    correcting rather than honouring.
  - **`--also` is a stop and report in v1.** Two sources of change (ADO's
    edit and the engineer's addition) arriving at one confirmation gate. Sync
    first, then `--also`.
  - **`--side` and `--user` are a stop and report.** An account or side change
    is never a side effect of a sync — see Step 1S item 5, which also refuses
    the same change arriving through the *ticket* rather than through a flag.
  - **An id with no existing artifacts is not a sync.** `npm run spec:locate`
    says so. Before reporting that `/gen-test <id>` without `--sync` is the
    command they wanted, **check the test case's heading format**: the id has
    to be the bare integer directly after `##`, followed by whitespace, and a
    heading that fails that is invisible to `spec:locate` while being valid to
    every other tool. Generating an id that already exists produces a
    duplicate spec, which is a far worse outcome than a second look.
- **`--common <component>`** — put this invocation's artifacts in
  `suites/common-components/<component>/` instead of under
  `sl/<side>/<module>/<feature>/`. It is **purely a destination flag**: it
  replaces the computed `<seg>` with `common-components/<component>` and
  changes nothing else about the pipeline.

  Use it to build a **cross-cutting component** — one many modules load, such
  as an employee search with its own filtrations — which belongs to no
  country, no side and no module. See "Common components" in @CLAUDE.md.

  ```
  /gen-test 127303 --common employee-search             # it has a ticket
  /gen-test --describe "..." --common employee-search   # it has none
  /gen-test 127303 --common employee-search --spec employee-search
  ```

  - **A common component is not restricted to `--describe`.** The rule is the
    same as everywhere else in this repo: fetch a real ADO ticket whenever one
    exists and treat it as the source of truth; fall back to `--describe` and
    a local id from `npm run gen:next-id` only when there is none. Being a
    shared component changes where the artifacts land, never where the truth
    comes from.
  - **Composes with every other flag.** `--spec`, `--chain`, `--also`,
    `--user` all behave exactly as documented above. The "every ID must
    resolve to the same `<seg>`" rule still holds and is trivially satisfied,
    since `--common` fixes `<seg>` outright.
  - **`--side` still applies, but not to the path.** No side appears in
    `<seg>`; `--side` continues to tell step 2 which side to capture the
    navigation chain from, and that side is recorded in `src/navigation.json`
    under `common-components/<component>` as usual.
  - **Never inferred.** Do not decide from a ticket's prose that a case is
    "about a shared component" — that is exactly the guessing the `**Uses:**`
    declaration exists to avoid. The engineer passes the flag or it does not
    apply.
- **`--side self|admin`** — unchanged. Ask rather than guessing if the ticket
  does not make the side obvious.
- **`--user <key>`** — the account the generated spec runs as. Skips the
  resolution below entirely. Use it when you already know the key.

Steps 1, 3 and 4 run **once per ticket**. Step 2 runs **once per invocation**
(all IDs share a `<seg>`, so they share a navigation entry). Steps 5, 6 and 7
run **once per invocation**, over all the tickets' raw specs together. A
`--describe` invocation carries exactly one described case, so steps 1, 3 and
4 run once — the ticketed path above still runs them per ticket.

1. **Fetch** — call the `azure-devops` MCP tool `wit_work_item` with the ID as
   an integer, requesting the `Microsoft.VSTS.TCM.Steps` field explicitly.
   Derive `<seg>` = `sl/<side>/<module>/<feature>` from the ticket (`<side>`
   is `self` or `admin`), and save the ticket as
   `suites/<seg>/test-cases/<name>.md`. If the side cannot be determined from
   the ticket, ask rather than guessing.

   With `--spec <stem>`, the tickets share one test-case file at
   `suites/<seg>/test-cases/<stem>.md` rather than one each. Each ticket
   becomes a section headed `## <id> — <title>`, with the ticket's own body
   below it, its headings demoted one level. **The id's position in the
   heading is load-bearing**: the linter's `assertion-count` rule reads these
   ids with `^##\s+(\d+)\s` to find each case's raw counterpart, so the id must
   be the bare integer directly after `##`, followed by whitespace. Only that
   much is matched — the ` — ` separator is this repo's convention for
   readability, not something the regex checks, and any separator would parse.
   The tolerance cuts the other way too, so write these headings deliberately:
   a `## 2026 Roadmap` heading anywhere in a test case is read as ADO id 2026
   and fails the lint as an id `suites/_generated/index.json` cannot resolve,
   and the same id heading written twice counts its counterpart's assertions
   twice into the raw total, so the combined spec is measured against a target
   it cannot reach. When appending to an existing `<stem>.md`, add sections at
   the end and leave the existing ones untouched.

   **With `--also "<extra>"`, the fetch is followed by an addendum pass —
   main session only, never a subagent**, because it ends in a human
   confirming a draft. It runs after the ticket is saved and changes nothing
   about the fetch itself.

   Write the addendum **inside that ticket's own `## <id> — <title>`
   section**, leaving the heading byte-identical — the id must stay the bare
   integer directly after `##`, or `assertion-count` can no longer resolve the
   section to its raw spec. Two things go in:

   - **A `**Also:** "<verbatim text>" — added by <author> on <date>` line**,
     directly under the heading, preserving what the engineer actually typed.
     `<author>` is `git config user.email`; `<date>` is today. Both come from
     the environment — never ask. This mirrors the `**Source:**` line a
     `--describe` case carries, and for the same reason: a year later nobody
     should have to wonder whether ADO said this.
   - **The drafted steps**, under a `**Added by engineer (not from ADO):**`
     marker, as table rows continuing the ticket's own numbering, each with
     the expected outcome it implies:

     ```markdown
     **Added by engineer (not from ADO):**

     | # | Step | Expected |
     |---|------|----------|
     | 4 | Remove the applied competency template | No longer listed; the template can be applied again |
     ```

   **Draft, print back, and wait for a yes — but do not interview.** The
   engineer wrote one line on purpose; turning it into the `--describe`
   conversation defeats the flag. Draft the steps and their expected
   outcomes yourself, print the whole section, and wait. This costs no extra
   round-trip: step 2 already stops to confirm the navigation chain and the
   account. If the engineer corrects the draft, redraft and print again —
   never generate against an addendum they did not agree to.

   **With `--describe "<intent>"`, step 1 is an interview instead of a
   fetch — main session only, never a subagent.** It asks a human questions and
   waits for answers. Same rule and reason as navigation capture below and the
   `test-data` skill: a subagent can ask nothing, so it would hang or invent.

   Ask one question at a time, not as a form:

   - **Which screen — only when there is no `--spec`.** With `--spec` the
     screen is already fixed by the target spec's path, so *state* the derived
     `<seg>` and its `src/navigation.json` entry back for confirmation rather
     than asking. Without `--spec`, ask, derive `<seg>`, and let a navigation
     miss route into step 2's normal user-assisted capture, unchanged.
   - **What must already exist** → the Preconditions table. Step 4 already
     treats a missing precondition as a stop-and-report rather than something
     to heal, so capturing it here pays for itself.
   - **The steps**, at whatever granularity the engineer offers; number them
     into the Test Steps table.
   - **The expected outcome, asked before any exploration.** Asking afterwards
     would only be asking the human to confirm whatever the app happened to
     do.
   - **Any specific data values**, or an explicit "you choose", which leaves
     them to the refactorer to invent and record as provenance.

   **The account is derived, not asked, when `--spec` is given** — a spec
   declares exactly one `test.use({ userKey })`, so a test appended to it must
   run as that account. Only a `--describe` run without `--spec` needs the
   `--user` flag or the registry filter below, exactly as the ticketed path
   does.

   **Allocate the id** with `npm run gen:next-id`, which prints the next free
   id from `cli/gen/local-id.mjs`'s `nextLocalId` — it scans both
   `suites/_generated/index.json` and the test-case tree, so it's the only
   thing that checks both. **Never hand-pick an id.**

   **The heading format**: `## 900000001 — <title> (local — no ADO ticket)`.
   The id must be the bare integer directly after `##` followed by whitespace,
   exactly as the ticketed path requires above, because `assertion-count`
   reads it with `^##\s+(\d+)\s`. The trailing marker is for humans, so nobody
   searches ADO for a number that was never there.

   **Add a `**Source:** described by <author> on <date>` line** where a
   ticketed case carries its ADO metadata. `<author>` is `git config
   user.email`; `<date>` is today. Both come from the environment — never ask.

   **Print the drafted test case back and wait for confirmation before
   generating anything.** A test case invented and silently acted on is the
   worst version of this feature: a plausible record no human agreed to. Same
   discipline as never writing an unconfirmed navigation entry.

   Registration in `suites/_generated/index.json` happens at step 3, once the
   raw spec exists and its filename is known — not here. Step 1 only
   allocates the id and writes it into the heading.

### Step 1S — with `--sync`, step 1 is a diff, not a fetch-and-write

**This runs in the main session, never a subagent** — it asks the engineer to
confirm a judgement, and a subagent can ask nothing.

1. **Locate.** Run `npm run spec:locate -- <id>`. On any refusal, stop and
   report it verbatim. Do not grep for the artifacts by hand.
2. **Fetch.** `wit_work_item` with the ID as an integer, requesting
   `Microsoft.VSTS.TCM.Steps` explicitly, exactly as a normal step 1 does.
3. **Diff against the stored section.** Compare the fetched ticket to the
   `## <id> —` section of the located test case, field by field and step by
   step. **The rendered table renumbers ADO's step ids** (135172's section
   notes ADO returning ids 2–10 rendered as 1–9), so a step inserted near the
   top shifts every row below it. Say so explicitly when it happens rather
   than reporting a whole-table rewrite.
4. **If nothing changed, stop and say so.** That is a useful answer, not a
   wasted run. Write nothing.
5. **If the account or the side moved, stop — before classifying anything.**
   This is read here, where the Preconditions diff actually is, rather than
   in Step 4S below, because a ticket whose Preconditions changed classifies
   as **Data / preconditions**, and that route skips pipeline steps 2–5
   entirely. Step 4S sits inside step 4's region, so on the one path most
   likely to trigger it, it is never reached: the Preconditions would be
   rewritten to name a different role while `test.use({ userKey })` kept the
   old account, with nothing reporting the divergence.

   A ticket whose Preconditions now name a different role is describing a
   different account, and an admin spec and a side-switching self spec must
   not share one — which side an account is on is server-side state that
   persists across logins, so a self-side spec changes where every admin spec
   using that account starts. Compare the fetched Preconditions against the
   stored section's, and against the account the spec actually declares in
   `test.use({ userKey })`. If they have diverged, **report both and stop**.
   Never rewrite `test.use({ userKey })`, and never rewrite the Preconditions
   to name a role the spec does not run as — that would leave the test case
   lying about the run. Changing the account is a deliberate human edit; see
   Step 4S below, which restates this refusal for step 4's own path.
6. **Classify the drift, and label it as a judgement.** Nothing is written
   yet. These bullets say what the chosen route **will** do, so the engineer
   has something concrete to weigh at the gate in item 7 — the only
   instruction here that tells you to write anything comes after they agree.
   One of:
   - **Wording** — reworded, clarified, typo-fixed; behaviour identical.
     This will update the test-case section only. Pipeline steps 2–5 will not
     run, but step 6 still will — both the spec run and
     `npm run lint:tests -- --require-specs` — and step 7 still reports.
     Proving the unchanged spec still passes is cheap and closes the
     "edited the wrong file" mistake.
   - **Data / preconditions** — same flow, different inputs or setup. This
     will update the test-case section and the data file **through the
     `test-data` skill**, never a hand edit. Pipeline steps 2–5 will not run,
     but step 6 still will — both the spec run and
     `npm run lint:tests -- --require-specs` — and step 7 still reports.
     Proving the spec still passes against its updated data is cheap and
     closes the "edited the wrong file" mistake.
   - **Behaviour** — a step added or removed, or an expected result that now
     describes something different. Pipeline steps 3–6 will run, scoped to
     this one case, **and the `## <id> —` section will be re-rendered from
     the fetched ticket**, preserving the two local blocks named below.
   - **Wholesale** — enough of all three that the case is unrecognisable.
     Pipeline steps 3–6 will run, regenerating the case, **and the
     `## <id> —` section will be re-rendered from the fetched ticket**,
     preserving the two local blocks named below.

   **The last two bullets have to say that, because no other step would.**
   Step 1 is the only step in the whole pipeline that ever writes a
   `## <id> —` section, and under `--sync` step 1 is this diff — steps 3–7
   write specs, data and summaries, never a test-case section. A behavioural
   sync that regenerated the test and left the section alone would leave the
   stored baseline describing the *old* requirement, so the next `--sync`
   would re-report a delta that had already been applied, with no way for the
   engineer to tell an already-applied delta from a new one. The whole design
   rests on that section being the stored prior state of the ticket, and on
   an incorrectly-routed sync being recoverable by simply re-running it.
   Neither holds if the baseline is never refreshed.
7. **Present the gate and wait.** Show, in this order: what changed in the
   ticket; how you classified it, stated as a claim the engineer can reject;
   what will be touched (the exact `test()` title, the data keys, the test-case
   section, and whether a browser will open); and **what will not be touched,
   naming the other tests in the file**. Write nothing until they agree.

   **Name every `manual` provenance conflict in the gate**, not afterwards.
   Read the data keys this sync would re-supply against the `_provenance`
   block of `suites/<seg>/data/<feature>.json`. A value marked `manual` was
   set by a human deliberately, through the `test-data` skill, after the
   pipeline ran — it is the one marker a human mints, and a sync that
   overwrote one and mentioned it in the step 7 summary would have already
   destroyed the value by the time the engineer read about it. List each
   conflicting key beside the ticket's new value and **ask which wins**. Step
   7 still reports what happened; this is where the engineer can still say no.

**Two things in that section are local, not ADO's, and must survive a sync
verbatim:** an `**Added by engineer (not from ADO):**` block written by
`--also`, and any dated `## Amendment — <date>` section written by
`/edit-spec` together with the `**Assertion floor:**` line that follows it.
Re-rendering the section from the fetched ticket and dropping these discards
the two records this repo keeps specifically to explain why a spec does more
than its ticket says.

2. **Navigation capture — main session only, never a subagent.** Before
   delegating to the generator, check `src/navigation.json` for the entry
   keyed `<module>/<feature>`.

   - **Entry found:** nothing to do, move on to step 3.
   - **No entry:** the menu chain has to be captured live, in *this* session,
     not handed to a subagent. A subagent cannot ask the user a question, and
     the sidebar is role-less `<div>`s (see CLAUDE.md's application facts) —
     an agent driving it blind with no human to confirm what it's clicking
     will loop. Instead:
     1. Open the browser with the saved session applied - the `.auth`
        state `ensureAuthFile` (`src/users/auth.ts`) mints or reuses for the
        resolved account, cached under `.auth/<environment>/`. There is no
        `setup` project; the session is minted on demand.
     2. Ask the user to navigate to the target page themselves and say when
        they're there. Do not attempt to find it yourself.
     3. Read `browser_network_requests` and `browser_snapshot` to reconstruct
        which side the screen is on and the exact chain of visible menu
        labels that reaches it. Establish separately whether the sidebar
        chain landed on the feature itself or on a **module home page** from
        which one further in-page link was clicked. That last click is
        `moduleNav` — it is a link outside `#phr-sidebar` (e.g. a module's
        own "Definition Wizard"), not a sidebar entry, and it is the one
        part of the chain a sidebar-only reconstruction will miss.
     4. **Confirm the reconstructed chain back to the user in plain text**
        before writing anything — e.g. "self side → Leave → Apply Leave,
        correct?" — and wait for their confirmation. Read the `moduleNav`
        click back as part of the chain when there is one, so the user
        confirms the whole route and not just its sidebar prefix.
     5. Only once confirmed, write the entry to `src/navigation.json` under
        `entries["<module>/<feature>"]`, with `side`, `menu` (the confirmed
        chain), `moduleNav` (only when such a click was confirmed — omit the
        key entirely otherwise), `verifiedOn` (today's date) and
        `capturedBy: "user-assisted"`.

   **Shared-component checkpoint — same handoff, before anything is
   generated.** This step is the one place in the pipeline a human is
   guaranteed to be present, so it is where the dependency on a cross-cutting
   component is settled:

   1. Ask the user whether this case exercises a **shared component** — an
      employee search, a date-range picker, anything many modules load rather
      than something this feature owns. Ask; never infer it from the ticket's
      prose.
   2. For each component they name, check that
      `suites/common-components/<component>/` exists.
   3. **If it does not, stop the pipeline and report.** Name the missing
      component and the exact command to build it first:
      `/gen-test <its own ADO id> --common <component>`, or
      `/gen-test --describe "..." --common <component>` when it has no
      ticket. **Do not proceed to step 3.** A component built after the fact
      is a component the refactorer has already re-derived locators for —
      which is the whole problem this ordering exists to prevent.
   4. If it does exist, record it in the test case, one line per component,
      beside `**User:**` and `**Navigation:**`:

      ```markdown
      **Uses:** common-components/employee-search
      ```

   The `missing-common-import` linter rule checks that declaration against the
   refactored spec's imports in step 6. It is a backstop, not the gate: a spec
   that drives a shared component **without** declaring it is not mechanically
   detectable, which is why the question above is asked of a human here.

   **The checkpoint is recursive.** A common component's own case may itself
   depend on another shared component; apply the same three steps at that
   level. A component's own spec never declares itself.

   **Never write a navigation entry that the user did not confirm.** A wrong
   chain silently sends every future test for this feature to the wrong
   screen, and nothing catches that except a human noticing. If the spec
   genuinely never navigates to a product feature (a framework self-test),
   this step doesn't apply — see the `**Navigation:** none` opt-out in
   `scripts/lint-tests.mjs` and `.claude/agents/test-refactorer.md` instead.

   **Resolving the account.** Two of the four key segments narrow the search:
   module from the target path, and environment from `TEST_ENV` *when it is
   set*. Resolve in this order:

   1. `--user` wins outright.
   2. List the registry, narrowing only by what you actually have:

      - `TEST_ENV` is **unset in this repo today**, and a missing value is
        "no environment filter", not "no accounts". Pass `--env` **only**
        when `TEST_ENV` has a value — `--env` with nothing after it is now a
        hard error rather than a silent empty result, which is exactly the
        bug this wording replaces.
      - Start with the module: `npm run users -- list --module <module> --json`.
      - **If that is `[]`, widen before concluding anything.** Run
        `npm run users -- list --json` and look at the module segment of
        what comes back. A registry module segment is **not** guaranteed to
        equal the module in the target path — an account is named for the
        product area it belongs to, which drifts from the directory name —
        and there is no catch-all segment to fall back on. So retry against
        whichever module segments the unfiltered list actually reports,
        `npm run users -- list --module <module> --json`, before reporting
        a gap. **Never write a specific account key into this file.** The
        registry is the only thing that knows what exists; a key copied
        into these instructions is stale the next time someone adds,
        renames or removes an account.
      - **Only if the unfiltered list is also `[]` — or holds nothing usable
        for this ticket — stop.** Report which module you looked for, that
        the unfiltered list held nothing usable either, and print the
        `npm run users -- add` command that would create the account. Never
        invent one: an invented credential fails at login with an error that
        never mentions the registry, twenty minutes into a run.

   3. Match the ticket's Preconditions text against the roles in the
      surviving set. A single match ("Login as a Performance Supervisor") is
      **preselected, not auto-accepted**: offer it as the default of the
      pick-list below and let the user confirm it. A module-filtered search
      can legitimately return exactly one *wrong* account — the result is
      non-empty, so the widen step above never fires — and a silent
      auto-accept turns that into a spec running as the wrong user with
      nothing in the run output saying so.
   4. Everything else — no role words, two matches, or several discriminated
      accounts — is the same pick-list with no default. This costs no extra
      interruption: step 2 already stops to confirm the navigation chain.

   Record the result in the test case as `**User:** <key>` with a
   `<!-- from-ticket -->` or `<!-- assumed -->` marker, and write
   `test.use({ userKey: '<key>' })` into `suites/_generated/seed.spec.ts`
   **before** dispatching the generator. The generator never chooses an
   account: it has no registry access and no way to ask a human.

   **With `--sync`, step 2 is normally skipped** — the feature is already mapped.
   Re-enter it only if the changed steps reach a screen `src/navigation.json` has
   no entry for, and then it is the full live, human-present capture as always. A
   sync is not an exception to "captured with a human or not at all."

3. **Generate** — delegate to the `playwright-test-generator` subagent. Tell
   it to write to `suites/_generated/<name>.spec.ts`, and tell it to call
   `generator_setup_page` with `project: 'generated'` and
   `seedFile: 'suites/_generated/seed.spec.ts'` — both arguments are
   required. Omitting `project` makes the tool fall back to the first
   top-level project in `playwright.config.ts` (`unit`, whose `testDir` is
   the repo root), which writes a stray `seed.spec.ts` at the repo root and
   then fails with "seed test not found". Do not edit that agent; customise
   its behaviour only through the prompt you give it.

   Also tell it, in the same dispatch prompt: **read `docs/app-quirks.md`
   before exploring** — it catalogs DOM behaviors of this app already
   established live (hidden duplicate elements, pre-rendered hidden dialogs,
   paired date inputs, and more), so a previously-solved quirk on a different
   screen doesn't have to be rediscovered blind. And: **the generator has no
   way to run the test it writes, so it has no way to detect its own
   failure — if, after a reasonable number of snapshots, you cannot locate
   the element a step needs, stop and report the blocked step verbatim. Do
   not guess a locator and do not keep exploring — a step you cannot perform
   is a handoff back to the main session**, which can drive the browser with
   a human present.

   **With `--also`, the case body you hand the generator is the ticket's
   steps plus the confirmed addendum steps**, and the prompt must say which
   are which — that the added ones came from the engineer, not from ADO. The
   generator explores them live exactly as it does a ticket's own steps; it
   just must not report them as ticket-sourced when it describes what it
   automated.

   **Generate one raw spec per ticket, always** — `--spec` combines at refactor
   time, never at generation time. Raw-per-ticket is what keeps step 4's heal
   blast radius at a single ADO case no matter how many cases share a
   refactored spec.

   After each raw spec is written, record it in `suites/_generated/index.json`
   as `"<id>": "<slug>.spec.ts"` — the id from the ticket, or the local id
   allocated in step 1 for a `--describe` case — creating the file if it does
   not exist and preserving every existing entry. The linter resolves these
   ids to raw specs through this map; without an entry it reports a
   `parse-error` rather than silently counting zero assertions, which would
   let a refactor drop assertions and still pass.

   **With `--sync`, generate only the case being synced**, into its existing raw
   spec filename from `suites/_generated/index.json` — the id already has an
   entry, so `index.json` does not change. The raw counterpart **must** be
   regenerated, because `assertion-count` measures the refactored spec against
   the sum of its raw counterparts; leaving the old raw spec would measure the
   new test against the old requirement.

4. **Verify raw & heal** —
   `npx playwright test suites/_generated/<name>.spec.ts --project=generated`.

   **Classify a failure before deciding what to do with it.** Read the
   failure — the assertion, the error message, the trace/screenshot if
   needed — and judge whether it looks like a genuine bug in the generated
   spec, or a **missing precondition**: an empty list where the test expected
   records to already exist, a control that should be enabled but renders
   disabled, a validation message naming a dependency that isn't satisfied
   (an approval waiting on a step that was never taken, say). If it's a
   missing precondition, **stop — do not delegate to the healer.** A healer
   pointed at a missing precondition doesn't know that; it will keep
   rewriting assertions and locators until an empty or disabled page
   satisfies them, which produces a spec that passes and proves nothing.
   Instead, record the discovered precondition in the test case's
   Preconditions table (`suites/<seg>/test-cases/<name>.md`) and report it —
   the environment needs the precondition set up, not the test rewritten.

   Otherwise, delegate to `playwright-test-healer` — but the healer's own
   vendor instructions say "repeat until the test passes cleanly," with no
   internal cap and no check for whether it's actually making progress. Every
   dispatch prompt to the healer MUST include this bounds block verbatim, on
   top of whatever else the dispatch says:

   > **Bounds for this dispatch — these override your standing instruction to
   > repeat until the test passes.**
   > 1. Read `docs/app-quirks.md` before you diagnose anything. Match your
   >    symptom against it first — most failures here are a known quirk on a
   >    new screen, not a new problem.
   > 2. **At most 3 fix→run cycles in this dispatch.** A cycle is: one edit
   >    to the spec, one `test_run`. When cycle 3's run fails, stop.
   > 3. **Same-signature short-circuit — stop at cycle 2, not 3, if you are
   >    not making progress.** After every run, note the failure signature:
   >    the failing test title, the step/line it failed at, the error class
   >    (timeout / strict-mode violation / assertion mismatch / navigation),
   >    the first line of the error message, and the locator involved — with
   >    timings, timestamps and `{{unique}}` values stripped. If the
   >    signature after your fix is identical to the signature before it,
   >    **stop immediately.** Do not try a third variation — an identical
   >    signature means your model of the failure is wrong, and a third
   >    guess against a wrong model wastes a run.
   > 4. **Stop conditions that are not "try harder" — hit any of these and
   >    stop on the spot:** the page snapshot shows a login screen (an
   >    environment/session failure, not a locator problem — do not rewrite
   >    locators against a login page); the failure looks like a **missing
   >    precondition** (an empty list where records were expected, a control
   >    that should be enabled rendering disabled, a validation message
   >    naming an unsatisfied dependency — do not rewrite assertions until an
   >    empty page satisfies them); the failure looks like a **product
   >    defect** (the app is doing something wrong, not the test — report it,
   >    a real bug must surface as a failure).
   > 5. **Flake protocol.** If a run passes after a change that shouldn't
   >    have mattered, or after a no-op, do not declare victory: run it twice
   >    more. Two green out of three is a flake, not a fix — classify and
   >    report it, do not silently absorb it.
   > 6. **Fixes you may not make.** Do not delete or weaken an `expect(` call
   >    (the `assertion-count` linter rule compares the refactored spec
   >    against this raw one later and will hard-fail anyway). Do not add
   >    `page.waitForTimeout()` or `networkidle`. Do not write `evaluate()`
   >    into the spec — the MCP evaluate tool is fine for investigation, but
   >    it may not land in code. Do not add `expect(...).toPass()` except
   >    under the conditions in CLAUDE.md's "Retrying a genuinely flaky
   >    step".
   > 7. **When you stop without a green test**, mark the test
   >    `test.fixme()` with a one-line reason, and report: every signature in
   >    order, what you changed each cycle, your best diagnosis, and which
   >    stop condition fired.
   > 8. **Always end your report with a `## Quirk candidate` section**, even
   >    if empty. Answer one question: was the root cause specific to *this*
   >    spec, or a general behavior of this application that any other spec
   >    on any other screen would also hit? If general, give the symptom as
   >    it appears in a failing run, the cause, the fix pattern, and your
   >    evidence for it.
   > 9. **Use `test_debug` for bounded inspection only, and always finish
   >    with a plain `test_run` as your verification.** A paused debug
   >    session is not a result.

   **If dispatch 1 stops without a green test:** the main session (this
   Claude Code session, not a subagent) reproduces the failure live with the
   MCP browser tools and works out the actual cause — this is the
   escalation path, since the healer subagent is pinned to a fixed model and
   cannot be swapped for a stronger one. The main session diagnoses; it does
   not hand-edit the spec itself.

   **Dispatch 2** — a *fresh* `playwright-test-healer` dispatch, carrying
   dispatch 1's full report ("already tried, do not repeat this") **and** the
   main session's diagnosis as an explicit instruction to act on. Same bounds
   block, but capped at **2 fix→run cycles** — there is nothing left to
   explore, only a known fix to apply.

   **Ceiling: 2 dispatches, 5 total test runs.** If dispatch 2 still fails,
   **stop the whole pipeline and report — do not proceed to step 5.**

4.5. **Capture the quirk** — runs after any heal (dispatch 1 or 2) that ended
   green. Read the healer's `## Quirk candidate` section from its report. If
   it names a general application behavior rather than something specific to
   this spec, append an entry to `docs/app-quirks.md` in the file's existing
   format: symptom as it appears in a failing run, cause, fix pattern,
   `**Established live:** <today>`, `**Permanent fix:** not yet promoted`, and
   a `**Class:**` of `app quirk`, `environment flake`, or `product bug`. If
   the class is `product bug`, this **must** be called out explicitly in the
   step 7 run summary — a defect the pipeline worked around is the single
   most important thing a human needs to see, and it must not be buried in an
   otherwise-green result. If the candidate section is empty or purely
   spec-specific, do nothing and move on.

### Step 4S — with `--sync`, two things are reported and never done

- **Never change the account or the side.** This is stated in full at **Step
  1S item 5**, where the Preconditions diff is actually read — read it there.
  It is restated here only because a behavioural or wholesale sync reaches
  step 4 and could discover the same thing late, in the shape of a raw spec
  that will not log in as the account the refactored spec declares. The rule
  is identical either way: report it and stop; never rewrite
  `test.use({ userKey })`.
- **Never re-baseline an assertion floor.** A recorded floor is carried
  forward as `floor + (rawSumNow - rawSumThen)`, and since step 3 regenerated
  the raw counterpart, a ticket that legitimately lost a step lowers the raw
  sum and the effective floor drops by the same amount on its own. If a sync
  ever appears to need `npm run spec:floor --apply`, that is the signal it
  should have been an `/edit-spec` restructure first.

5. **Refactor** — delegate to the `test-refactorer` subagent (ours — see
   `.claude/agents/test-refactorer.md`) with the raw spec path, the test-case
   path, and `<seg>`. Its navigation contract drops whatever navigation the
   raw spec did — the auto-fixture built in step 2's map replaces it.

   **When the test case carries `**Uses:**` lines, the dispatch prompt must
   name them — and hand over the page object's API, not just its name.** For
   each declared component, read
   `suites/common-components/<component>/pages/<component>.page.ts` and put in
   the prompt:

   - the import path, `@common/<component>/pages/<component>.page`, and
   - its **public API surface** — the exported class name, its public locator
     names and its method signatures.

   The name alone is not enough. The generator explores the live app blind to
   all of this and writes raw interactions for the shared component just as it
   does for everything else — deliberately, because the raw spec must run and
   pass in step 4 and is the `assertion-count` baseline. So the refactorer is
   the layer that swaps those interactions for the existing page object, and
   it can only recognise which chunk of an undifferentiated raw spec *is* the
   component by matching against a concrete list of what that component
   already exposes.

   **Also tell it where assertions go.** `assertion-count` counts `expect(` in
   the **spec file only**. Assertions moved inside a common page object vanish
   from that count and fail the rule with a message that reads as "you dropped
   assertions" when they were merely relocated. A common page object exposes
   locators and actions; the `expect(`s stay in the spec.

   **Never refactor a spec that still contains `test.fixme(` or
   `test.skip(`.** A fixme'd raw spec is the healer's declared failure, and a
   skipped test satisfies both the run in step 6 and the `assertion-count`
   rule while proving nothing. If the raw spec is fixme'd, stop and report
   instead of proceeding.

   With `--spec <stem>`, dispatch the refactorer **once** with every ticket's
   raw spec path, the shared test-case path, `<seg>`, the target stem, and
   whether `--chain` was given. It produces one
   `suites/<seg>/specs/<stem>.spec.ts` holding one `test()` per raw spec. If
   that file already exists, it **appends** the new tests and leaves every
   existing test byte-identical — see the append contract in
   `.claude/agents/test-refactorer.md`.

   **If any ticket's raw spec is `fixme`'d, do not refactor any of them.** A
   combined spec assembled from a mix of passing and failed cases hides the
   failure inside an otherwise-green file. Report the failed ticket and stop.

   **With `--sync`, the refactorer performs a single-test splice**: it rewrites
   the body of the one `test()` carrying `// ADO <id>` and returns the spec
   otherwise byte-identical. That is verified mechanically, and the proof
   needs a snapshot that only exists if you take it first.

   **Before dispatching the refactorer**, copy the spec as it stands:

   ```bash
   mkdir -p .gen-test-scratch
   cp suites/<seg>/specs/<stem>.spec.ts .gen-test-scratch/before.spec.ts
   ```

   The refactorer edits the spec **in place**, so the pre-splice text is gone
   the moment it returns; there is no other way to recover it, and without it
   the one mechanical proof that the sync stayed in its lane cannot run at
   all. `.gen-test-scratch/` is gitignored, exactly as `/edit-spec`'s own
   `.edit-spec-scratch/` is, and for the same reason: an unrelated
   `git add -A` between the copy and the cleanup would otherwise commit a
   duplicate of a spec.

   **After the refactorer returns**, run the check:

   ```bash
   npm run spec:splice-check -- suites/<seg>/specs/<stem>.spec.ts .gen-test-scratch/before.spec.ts <id>
   ```

   It prints the resolved test title and either a clean result or the exact
   violations. **On a non-zero exit, print the violations and stop; never
   hand-fix the spliced spec** — a hand fix is precisely the collateral edit
   the check exists to catch. Delete `.gen-test-scratch/` when the step is
   over, whichever way it went.

   Run it through `npm run`, never as a `node -e` one-liner. Git-bash on this
   host rewrites POSIX-looking paths inside a `-e` string into Windows ones
   and mangles them — the same trap `.claude/skills/edit-spec/SKILL.md`
   documents for its own gate script.

   The check refuses more than a stray neighbouring edit. It also refuses the
   spliced test losing or swapping its `// ADO <id>` marker, moving position,
   changing `describe` membership, gaining or losing `.serial`, and being
   chained into *or* out of the chain's shared record. A sync rewrites one
   body; it never restructures in either direction. In particular
   `verifySplice` never declares `unchain`, so a regenerated test that stopped
   reading the chain's shared variable is **refused** — deliberately: send the
   engineer to `/edit-spec` to restructure on purpose, then re-run the sync.

6. **Verify refactored** —
   `npx playwright test suites/<seg>/specs/<name>.spec.ts --project=SL` and
   `npm run lint:tests -- --require-specs`. Both must pass. `--require-specs`
   is what stops a green lint run on a tree the linter never found your spec
   in from counting as proof; still read the `Scanned N spec file(s).` line
   and confirm the new spec was among them — see the "Proof" section of
   `test-refactorer.md`.

7. **Run summary** — once step 6 passes, read
   `provenanceOf(specPath)` (from `@utils/load-data`, or the raw
   `suites/<seg>/data/<feature>.json`'s per-case `_provenance` block
   directly) for the spec just written, and print a summary of what the
   pipeline made up rather than took from the ticket. Group by provenance
   value, e.g.:

   ```
   ── rating-methods ────────────────
     createValid        2 invented   methodName, items
     createValid        1 assumed    type → "Numeric" (of 3 options)
     createWithoutType  2 invented   methodName, items
     ▶ review suites/<seg>/data/<feature>.json before trusting these tests
   ```

   `provenanceOf(specPath)` returns a multi-case spec's block already
   flattened to `"<caseKey>.<field>" -> <how>`, so group on the part before the
   first dot. A single-case spec's keys have no dot and group under the spec
   itself, exactly as before.

   Omit the summary (or state plainly "nothing invented — every value came
   from the ticket") when `_provenance` is empty or the data file has none.
   This is the one place the pipeline surfaces its own guesses; report
   success only after printing it.

   If a heal occurred, also print the heal record so an unattended run's own
   output shows where it got stuck rather than just a pass/fail: how many
   dispatches and cycles were used, each failure signature in order,
   whether a `test.fixme()` was ever left (even if later resolved), and any
   `docs/app-quirks.md` entry captured — with its `Class` called out loudly
   if `product bug`.

   **If the invocation used `--also`, print the addendum verbatim in the
   summary**, with the steps it became. Those steps were never in the ticket,
   so they belong in the same part of the report as an invented value: the
   one place the pipeline states what it did not take from ADO. Add the
   re-runnability caveat when the addendum was cleanup — it runs as test
   steps, so a failure before that step still leaves the record behind.

   **If the invocation used `--describe`, say so plainly in the summary.**
   With no ticket, the description is the only statement of intent, so a
   buggy application plus a thin description lets the generator encode the
   bug as correct. The pipeline already reports what it invented; this is the
   same discipline applied to where the expectation itself came from.

   **With `--sync`, the summary also reports:** which drift kind was applied,
   which `test()` changed and which were left alone by name, whether a browser
   opened, and every provenance marker that moved. A value that was `invented` or
   `assumed` and is now stated by the ticket becomes `from-ticket` — report that,
   it means the ticket got more specific. A value marked `manual` is **never
   overwritten silently**: report the conflict and ask.

Report success only when step 6 passes.

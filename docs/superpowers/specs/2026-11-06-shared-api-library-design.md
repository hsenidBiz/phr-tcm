# Shared API library across environments — design

Date: 2026-11-06. Status: approved by the owner 2026-11-06.

## Intent

API templates, flows and fixtures are kept separately for each Auto Run
environment today (`templates/`, `flows/`, `fixtures/` under
`<appdata>/autorun/envs/<env-id>/`). Environments of the same application
usually expose the same endpoints, so every new environment rediscovers what
another one already knows. They are not always the same, though, so a library
item that works on one environment cannot be trusted on another.

The owner wants one library that every environment sees. Each item is
**confirmed** or **unconfirmed** separately on every environment. The
assistant may use an unconfirmed item. If it works, it becomes confirmed on
that environment. If it fails, the assistant works out the version that works
there and saves it, and that version replaces the item for everyone.

Separately: copying Auto Run data from another environment must never copy
accounts. They are quick to set up and usually differ per environment.

## Goals

- One library of API templates, flows and fixtures per org/project, shared by
  every environment.
- Confirmation is tracked per environment, per item.
- The assistant can use unconfirmed items. On failure it rediscovers and
  saves a working version.
- Saving a changed item replaces it everywhere. It is confirmed only on the
  environment that proved it, and unconfirmed on every other.
- Environment copy and backup import stop carrying templates, flows and
  fixtures, and never carry accounts.

## Non-goals

- Per-environment variants or overrides of an item. The owner rejected this
  (option (a)) in favour of one version everywhere.
- Keeping or restoring an item's earlier versions after it is replaced,
  beyond the one-time move described below.
- Changing how accounts are stored.
- Sharing anything else (scripts, areas, components, quirks, run history).

## Anti-goals: it fails if…

- Something marked confirmed on an environment never actually worked there.
- An unconfirmed item fails in a run and the failure reads as a defect in the
  application rather than "not yet working here".
- A copy or import brings one environment's accounts or credentials into
  another.
- The assistant keeps retrying an item it has already given up on for that
  environment.

## Decisions (owner)

1. One shared library. A replaced item replaces it everywhere (option (b)).
2. Per-environment confirmation. The assistant tries unconfirmed items, and
   rediscovers and replaces them when they fail.
3. Environment copy never copies accounts.

## Decisions (mine, approved in conversation as a whole)

- **What confirms an item on an environment:**
  - Template: a successful proving run there. This is the existing
    `prove_api_template` rule, and an explore-session save still counts,
    as it does today.
  - Fixture: a successful build (fixture run) there.
  - Flow: computed, never stored. A flow is confirmed on an environment when
    every template it names is confirmed there.
- **A failure on an environment** never changes the item. It records that the
  item failed there, so the screens and the assistant see "failed here". The
  next successful prove or build there clears it.
- **Replacing an item** (saving it with different content) makes it confirmed
  on the environment that proved it and unconfirmed everywhere else. That
  includes clearing their "failed here" marks, because the new version has not
  been tried there.
  - Saving identical content, such as a re-prove, only updates the proving
    environment's confirmation.
- **Normal Auto Run with an unconfirmed fixture as a case's setup:** the
  fixture still runs.
  - If it succeeds, it becomes confirmed on that environment.
  - If it fails, the case is **Blocked** with the reason "this fixture has not
    worked on this environment yet". The Needs attention panel points the
    person to the assistant.
  - A plain run never rewrites an item.
- **Accounts:** a fixture keeps naming an account key. An environment with no
  account under that key fails the fixture there, and that counts as a
  failure. The fix is to add the account on that environment, not to edit the
  shared fixture. Account keys stay per environment, as today.
- **Screens** (API Templates: Templates, Flows, Fixtures tabs; the Auto Run
  readiness panel; the flow page):
  - an unconfirmed item shows **Unconfirmed here**;
  - a failed one shows **Failed here**;
  - a confirmed one shows nothing new.
  - The existing "unproven" wording becomes "unconfirmed here".
- **Run history stays per environment.** `<id>.runs.json` and fixture run
  history stay under the environment root. Only the definitions move.
- **Export and import of templates and flows:**
  - Export writes the shared definitions without any confirmation state.
  - Import writes into the shared library. An imported item that is new, or
    that differs from the existing one, is unconfirmed on every environment.
  - An item that is identical keeps its confirmations.

## A confirmed item that later fails (owner, 2026-11-06)

- A confirmed item stays confirmed when a run of it fails on that
  environment. The failure is recorded and shown the way a failed run is
  today. For a case's setup, the case is Blocked with today's fixture-failure
  sentence, not the "not worked here yet" one.
- The assistant replaces a confirmed item only after a fresh try of the
  current version also fails there. Only then is it marked "failed here" and
  rediscovered.
- "Failed here" therefore arises only from:
  - an unconfirmed item failing; or
  - a confirmed item failing again on that retry.

## Settled after the builder check (my calls)

- **Save and run gates mean "proven on some environment".**
  - Saving a fixture still refuses a step whose template has never been
    proven anywhere. That is today's `proven` check, kept global.
  - Starting a fixture run uses the same global check. A shared fixture can
    therefore run on a fresh environment, as the setup rule above requires.
  - The per-environment state drives only the marks, the assistant's
    listings, and what confirms or fails.
- **A successful fixture build also confirms each template it ran** on that
  environment. A real build exercised them, so a flow built from those
  templates reads confirmed there too.
  - A failed build marks only the fixture as failed here. The failure may
    come from data rather than from the template.
- **Replacing a template clears the confirmations of every fixture that uses
  it** on every environment other than the one that proved it. Without this,
  a fixture would read confirmed on an environment where the new template
  never ran, which breaks the first anti-goal.
- **Backups:**
  - A backup made after this change includes the shared library, and
    restoring it restores the library.
  - A backup made before it holds templates, flows and fixtures inside each
    environment's folder. Restoring it folds them in by the one-time move's
    rules, so nothing is silently lost.
- **Losing versions from the move** go to their own kept folder under the
  shared root (`removed-api/<ms>/`, by environment and id). It is purged
  after 30 days. It is never listed as a source for Copy Auto Run data, and
  it is never treated as an environment.

## The one-time move

The update folds each environment's `templates/`, `flows/` and `fixtures/`
into the shared library. This includes removed environments still kept, so
that a copy from one still has nothing API-related to bring.

- An id held by only one environment, or held with identical content by
  several: it moves as is. It is confirmed on each environment where it was
  proven or built successfully before the move.
- An id held with **different content** by several environments: the version
  most recently proven or built wins.
  - The winner is confirmed only on the environments whose own version was
    identical to it and had worked there. It is unconfirmed on all others.
  - Losing versions are kept for 30 days in the removed area, the same way a
    replaced environment's data is kept, so they can be recovered by hand.
- The move is crash-safe and idempotent like the existing environment
  migration (`roots.rs`, `MIGRATION_NOTE_FILE`): built aside, then switched in.
- Owner accepted the risk that the newest version can replace one that worked
  on another environment. That environment then shows Unconfirmed here, and
  the assistant re-proves or rediscovers it.

## Environment copy and accounts

- `PER_ENV_ITEMS` loses `templates`, `fixtures` and `flows`. They become
  shared items. Environment copy (`env_copy.rs`) and backup import therefore
  stop carrying them.
- Accounts are already shared-root data, filed by environment
  (`accounts::load_accounts_for`), and the copy already leaves them alone.
  This design keeps it that way and pins it with a test: after a copy, the
  target's accounts are unchanged and the source's are not visible under the
  target.
- The Copy Auto Run data dialog's note changes:
  - it no longer lists API templates;
  - it says plainly that accounts are not copied;
  - it says API templates, flows and fixtures are shared by every environment
    already.
- The single-case copy (`copy_case`) keeps reporting missing accounts and
  never copies them. That is unchanged.
- `COPY_NOTHING_TO_COPY` and `is_empty_env` no longer count templates, flows
  or fixtures.

## The assistant

- `list_api_templates` and the fixture/flow listings report, per item, its
  state on the **active** environment: confirmed, unconfirmed or failed here.
  When failed, they include the last failure's step and detail.
- Running or proving an unconfirmed template is allowed. `run_api_fixture` on
  an unconfirmed fixture is allowed.
- The assistant guide (`api_templates/guide.rs`) and the MCP tool
  descriptions say the rule:
  - an unconfirmed item is worth trying first;
  - if it fails, work out the version that works here and save it (this
    replaces it for every environment);
  - if that cannot be found, leave the item alone. It stays "failed here" and
    is not retried in this job.
  - `UNPROVEN_FOR_ASSISTANT` is reworded for the per-environment meaning.
- The flow gate (`flow_problems`) and the save checks (`gate.rs`) are
  unchanged in what they require. They read definitions from the shared
  library.

## Storage (builder's call, with constraints)

- Definitions live under the shared root, keyed by org/project slug as today
  (for example `<appdata>/autorun/templates/<project_slug>/<id>.json`).
- Confirmation state is kept per environment and per item. Storing the
  per-environment state separately from the definition (for example a state
  file under each environment root) is preferred: replacing a definition
  should only have to clear other environments' state, and removing an
  environment should take its state with it.
- `ApiTemplate::proven` today is a single proof inside the definition. The
  builder decides whether it stays as "last proof of this version" or moves
  entirely into per-environment state. Either way, a template file written
  before this change must still load.
- Writes go through the existing locks (`roots::switch_lock`, guarded writes).
  Two environments are never active at once in one app, but the assistant and
  a run can write concurrently.

## Constraints

- No HTTP DELETE to Azure DevOps (this touches none).
- Every Rust test goes in `src-tauri/tests/suite/`.
- Regenerate `src/bindings.ts`; never hand-edit it.
- User-facing text names no paths or URLs.
- The changelog entry is end-user wording.
- The help guide (`docs-site/src/content/api-templates.ts`, `auto-run.ts`, the
  environments content) is updated, and screenshots are retaken where the
  marks show.

## Left to the builder

- Exact file layout of the per-environment state.
- How "most recently" is read during the move. Use the run history's newest
  successful prove or build, falling back to the file's modified time.
- Wording of the marks and tooltips, within the terms above.
- Whether the readiness panel counts unconfirmed fixtures separately from
  failed ones.

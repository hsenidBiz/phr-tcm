# Auto Run: fixtures, script setup and cleanup of test-made drafts (part 5)

Design agreed with the owner on 2026-10-06. It covers items 8, 9 and 10 of
`docs/autorun/backlog-2026-10.md`:

- **Fixtures:** a fixture makes a draft the same way every time. Rebuild
  makes a fresh one when a shared draft is damaged.
- **Setup:** a script that needs its own draft gets it from a fixture, in
  seconds, instead of clicking through Copy from Previous.
- **Cleanup:** drafts the tests made can be removed on demand.

Item 11 (templates and Auto Run on the same account at once) is already done
by the per-account lease in `autorun/lease.rs`.

## Owner decisions

- **Rebuild** makes a fresh draft, and scripts follow it through the fixture's
  outputs. The damaged draft is left for cleanup. Rebuild needs no delete.
- **A script's own draft** is made fresh on every run of the case.
- **Approving a setup:** the person approves it once, in the script editor.
  Changing the setup or its fixture clears the approval.
- **How cleanup deletes:** through a proven delete API template, and only for
  drafts recorded as test-made.

**Approach:** fixtures are a new kind of item on the API Templates screen. They
reuse the template runner, its signed-in browser, its sign-in and the
per-account lease. Flows are unchanged.

## 1. Fixtures

### What a fixture holds

A fixture is stored at `fixtures/<slug>/<id>.json` beside the templates. It
holds:

- `id`, `name` and `account` (an Auto Run account key, as a template run
  takes);
- `steps`: an ordered list of `{ template, params }`. Each param value is
  text that may hold these placeholders:
  - `{{steps.<n>.<capture>}}`: a value captured by an earlier step, where
    `<n>` starts at 1;
  - `{{now:<format>}}`: the run's start time, with the format letters
    `yyyy MM dd HH mm ss`;
  - `{{prefix}}`: the environment's test name prefix (section 3).
- `outputs`: a map from an output name to `{{steps.<n>.<capture>}}`.
- `creates`: a list of `{ kind, id, name }`. Each entry is a pair of
  placeholders saying what the fixture makes, for example `{ kind: "cycle",
  id: "{{steps.1.cycle_id}}", name: "{{steps.1.cycle_name}}" }`. These are
  what the record of test-made drafts keeps (section 3).

### Rules at save

A fixture is refused at save with one sentence per problem:

- `step <n>: template <id> is not proven`
- `step <n>: template <id> deletes, and a fixture never deletes`. This
  applies to a template with any `delete` effect.
- `step <n>: {{steps.<m>.<capture>}} does not come from an earlier step`.
  This applies when `m >= n` or step `m` captures no such value.
- `output <name>: {{steps.<m>.<capture>}} does not come from a step`
- `a fixture needs at least one step`
- `a fixture with creates must use {{prefix}} in a step's params`. This
  applies when `creates` is not empty and no step param holds `{{prefix}}`.

A name can only be checked once it exists. So at run time, a made thing
whose name does not start with the prefix is still recorded, and the run
reports `<kind> <name> does not start with the test prefix, so Clean up will
not find it`.

### Running a fixture

- **How steps run:** in order, through the template runner, in one fresh
  signed-in browser as the fixture's account, under the account lease. The
  lease waits up to 60 s, as a template run's does.
- **Stopping:** the run stops at the first failed step, with the template
  runner's own failure sentence prefixed by `step <n>: `.
- **The fixture's record:** a run is kept in `<id>.runs.json`, newest first,
  holding 20 runs, as templates do. A run records its outputs, its start time
  and its outcome.
- **Current outputs:** the outputs of the newest successful run are the
  fixture's current outputs.
- **Rebuild** runs the fixture again. When the run succeeds, its outputs
  become current. When it fails, the current outputs are kept, and the
  failure is shown.
- **Recording what it made:** whatever the run made is recorded as test-made
  (section 3), even when a later step fails. A half-made draft is still
  cleaned up.

### The Fixtures tab

A Fixtures tab on the API Templates screen, beside Templates and Flows. It
lists each fixture with:

- its name and steps;
- its current outputs;
- its last run, with its time and outcome;
- the buttons Run (first build) or Rebuild.

The tab also holds Clean up test-made drafts (section 4).

### The assistant's tools

Three new tools, offered with the API template tools and behind the same API
writes switch:

- `save_api_fixture`
- `run_api_fixture`
- `list_api_fixtures`

The guide (`api_templates` guide text) says:

- build a fixture from proven templates;
- name what it makes with `{{prefix}}`;
- use Rebuild, never a hand fix, when a shared draft is damaged.

## 2. Scripts that use fixtures

### A shared draft

A script step's value may hold `{{fixture.<id>.<output>}}`, which is
replaced with that fixture's current output when the case starts.

- **Never built:** a fixture with no successful run blocks the case with
  `fixture <name> has not been built - run it from API Templates, Fixtures`.
- **Unknown names:** an unknown fixture id or output name is refused when the
  script is saved: `{{fixture.<id>.<output>}}: there is no such fixture
  output`.

### A script's own draft

- **Declaring it:** `CaseScript` gains an optional `setup: { fixture: <id>
  }`. It is serialised only when present, so old scripts load unchanged.
- **When it runs:** on every run of the case, unattended or supervised, after
  the preconditions and before the case's browser signs in. The setup runs
  the fixture as in section 1. Its browser is closed and its lease released
  before the case signs in.
- **Using its outputs:** the run's outputs are available to the case's steps
  as `{{setup.<output>}}`.
- **A failed setup** blocks the case with `setup failed: <the fixture's
  failure sentence>`.
- **The assistant's replay to step N (part 4)** runs the setup too, because
  the case needs its draft. It still needs the approval.
- **Checks at save:**
  - `setup: there is no fixture <id>`
  - `{{setup.<output>}}: fixture <name> has no output <output>`

### Approval

- **Where approvals live:** in the app's store,
  `approvals/<case id>.json`, never in the script file. No tool can write
  them, so the assistant cannot approve its own setup.
- **What an approval holds:** a fingerprint (SHA-256) of the setup together
  with the fixture's `steps`, `outputs`, `creates`, `account`, and each
  step's template body. When any of these changes, the fingerprint no longer
  matches and the approval no longer counts.
- **Without an approval:** a case with a setup that is not approved, or whose
  fingerprint no longer matches, is Blocked with `setup not approved - approve
  it in the script editor`. Nothing runs and nothing signs in.
- **The script editor** shows a "Setup" section when the script has one. It
  holds:
  - the fixture's name;
  - its account;
  - each step as `<template name>: <params>`;
  - what it creates;
  - `Approve setup`, or `Approved <date>` with `Withdraw approval`. A
    fingerprint that no longer matches shows `Changed since you approved it`
    and `Approve setup` again.

## 3. The record of test-made drafts

### The test name prefix

- **Where it is set:** each environment gains a "Test name prefix", set on
  the environment's card.
- **Its default:** `AUTOTEST`.
- **Its limits:** 3 to 20 letters, digits or `-`.

### What the record holds

- **Where it lives:** `test-made.json` in the Auto Run store holds one entry
  per thing a fixture or setup run made. An entry is `{ environment, kind,
  id, name, created_at, fixture, run_id, case_id?, status }`.
- **Statuses:** `status` is `present`, `deleted` or `delete failed: <the
  reason>`.
- **Writing it:**
  - Only the fixture runner writes new entries.
  - Only Cleanup changes a status.
  - No tool can add, edit or remove entries.

## 4. Cleanup of test-made drafts

### How a person cleans up

1. **Start:** press "Clean up test-made drafts" on the Fixtures tab. Cleanup
   only ever starts from the app and by a person. The assistant has no tool
   for it.
2. **Choose:** pick the environment, the prefix (defaulting to the
   environment's) and "older than" in days (default 7, minimum 1).
3. **Preview:** the entries that:
   - are in the record with status `present` or `delete failed`;
   - are for that environment;
   - have a name starting with the prefix (compared ignoring case);
   - are older than the age.

   Each line shows the kind, name, id, age and the fixture that made it.
   Every line is ticked to start with.
4. **Confirm:** the dialog says `Delete <n> drafts from <environment>? This
   cannot be undone.`, with the buttons Delete and Cancel.
5. **Delete:** each ticked entry is deleted, one at a time, by the delete
   template for its kind (see "Delete templates" below).
   - Each result is shown as it comes, and written to the record.
   - A Stop button ends the cleanup between deletes.
   - Deleting runs in one signed-in browser as the delete template's
     account, under the lease.
6. **No delete template:** a kind with no proven delete template shows its
   entries in the preview, marked `no proven delete template for <kind>`, and
   they cannot be ticked.

### Delete templates

A template is a delete template when any of its steps has the `delete`
effect. For a delete template:

- **Its subject:** it declares the `kind` it deletes, and takes the record's
  id as the param `{{id}}`.
- **Who may run it:** only Cleanup. `run_api_template` and fixtures refuse it
  with `template <id> deletes, and only Clean up test-made drafts runs a
  delete template`.
- **Proving it:** `prove_api_template` only accepts an `id` that is in the
  record for that kind and environment with status `present`. Otherwise it is
  refused with `a delete template is only proven on a draft the tests made`.
  A proof that succeeds marks that entry `deleted`.

## 5. Safety

- **What Cleanup can delete:** only a record entry. It is never a search of
  the app under test. Cleanup always needs a person's preview and
  confirmation.
- **What a fixture or setup is:** only proven templates, run under the
  account lease. A setup also needs the person's approval with a matching
  fingerprint.
- **Azure DevOps:** none of this touches Azure DevOps. The rule that only
  `ado/deletion.rs` issues a DELETE to Azure DevOps is unchanged. Deletes
  here go to the application under test through its own handlers, inside the
  signed-in browser.
- **What it records and logs:** no passwords, cookies, hosts or query strings
  in any record, event, log line or sentence.

## 6. Out of scope

- Repairing a damaged draft in place.
- Deleting anything that is not in the record, including the drafts made by
  hand before this feature.
- Scheduled or automatic cleanup.
- Running a fixture to carry out a part 6 reset point.

## 7. Testing

### Rust (`tests/suite` only, with the fake browsers)

- **Fixture save:** every refusal sentence.
- **Value passing:** between steps, with `{{now}}` and `{{prefix}}`.
- **Stopping:** a run stops at a failed step.
- **Runs and outputs:**
  - the run record holds 20;
  - current outputs come from the newest success;
  - a Rebuild that fails keeps the outputs.
- **The test-made record:**
  - written for a half-made draft;
  - no tool can write it.
- **Scripts:**
  - `{{fixture...}}` replaced, never built, and unknown;
  - setup runs before sign-in, fails as Blocked, and its outputs are used.
- **Approval:**
  - missing, fingerprint changed by each of the setup, the steps and a
    template body;
  - nothing signs in while not approved;
  - the approval file cannot be written through a tool.
- **Cleanup:**
  - the age, prefix, environment and status filters;
  - a kind with no delete template;
  - Stop;
  - results written.
- **Delete templates:**
  - refused by `run_api_template` and by fixtures;
  - proving refused off the record, and allowed on it.
- **Old files:** old scripts and old run files load unchanged.

### vitest

- **The Fixtures tab:** list, Run and Rebuild.
- **The script editor's Setup section:** Approve, Changed since you approved
  it, and Withdraw.
- **Cleanup:** the filters, the preview, the untickable lines, the confirm
  sentence, the results and Stop.
- **The environment card:** the prefix field and its limits.

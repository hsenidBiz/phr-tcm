---
name: edit-spec
description: Use when changing a test that already exists in suites/**/specs/<stem>.spec.ts - "make these run in order against one record", "delete this test", "chain these together", "reorder these tests". The only sanctioned way to edit a refactored spec; never hand-edit one.
---

# Editing a spec

A refactored spec under `suites/**/specs/<stem>.spec.ts` is generated and
policed by machinery a QA engineer has no reason to read: `assertion-count`,
`orphan-case`, `provenance-mismatch`, and the chain's closure-variable rule
from `test-refactorer.md`. Hand-editing one means silently fighting four
linter rules and a data-file contract you can't see. This skill is the front
door: it shows the spec as plain English, applies the change through the
`spec-editor` subagent, verifies it mechanically, and proves the result back
in the same plain English it opened with.

**R1 scope is restructure and remove only** — reorder tests, chain them into
`test.describe.serial` around one shared record, or delete a test and cascade
its data and test-case section. Changing what a test *does* (new steps, new
assertions, new values) needs live DOM verification and isn't built yet;
adding cleanup/teardown needs fixture machinery that doesn't exist yet either.
See "Out of scope" below — say so and stop rather than improvising either one.

This is **not** the skill for values inside `data/<feature>.json` — that is
`test-data`. And it is not how a *new* ADO case gets added to a spec — that is
`/gen-test <id> --spec <stem>`, which appends. This skill only reshapes tests
that are already there.

## Two rules that are never bent

**1. Main session only. Never delegate this to a subagent.** This skill asks
the human which test they meant, whether a trade-off (like `.serial`'s
failure semantics) is acceptable, and what they observed when something
failed. A subagent can ask nothing, so it would either hang or invent an
answer. Same rule, same reason, as navigation capture in `/gen-test` and the
`test-data` skill.

**2. Visualise first, always.** No path through this skill begins with a
proposed change — not even when the request sounds unambiguous ("delete the
second test"). Run `npm run spec:show -- <stem>` and print it verbatim before
anything else. The render is how the human confirms you landed on the right
test before anything is written, exactly as the grid is in `test-data`.

## The five beats

```
1. RENDER    npm run spec:show -- <stem>, printed verbatim
2. POINT     they confirm which tests; you analyse and ask
3. PROPOSE   restatement + blast radius + before/after, nothing written
4. APPLY     checkpoint -> dispatch -> enforce radius -> run -> lint
5. RENDER    spec:show again; they confirm it says what they meant
```

## Beat 1 — Render

```bash
npm run spec:show -- <stem>
```

If the stem is unknown or ambiguous the command says so and lists what
exists (by stem, or by `<seg>/specs/<stem>.spec.ts` when the stem alone is
ambiguous) — show that and ask, never guess which one they mean.

If the outline reports that two tests share a title, **stop here.** Titles
are the only handle this skill has on a test — the gate compares before and
after by title, so two tests called the same thing cannot be told apart and
neither one has an addressable blast radius. Tell them plainly: "there are
two tests both called `X`; one of them has to be renamed by hand before I
can safely touch either." Do not proceed and do not offer to pick one.

Print the outline verbatim. It is derived from the spec's own AST — title,
`// N.` step comments, the `loadCase` key and its resolved values, the
declared `userKey`, and the navigation chain — so it cannot lie about what
the code does the way `test-cases/<stem>.md` can drift from it. Do not
summarise it or reformat it into your own table.

## Beat 2 — Point

Their phrasing is not a test title. Match what they said against test
titles, `// ADO <id>` comments, `// N.` step comments, and the data values
the render shows.

- **One match** — mark it and move to beat 3.
- **Several** — list them and ask. **Never pick.**
- **None** — say so, re-render, and invite them to point at it.

Then ask the questions only a human can answer — an agent has no basis to
guess any of these:

- **Which value survives a collapse.** Chaining two independent tests around
  one record usually means two record names become one (the worked example
  is two rating-method names collapsing into one). Ask which survives; it is
  a data decision, not a structural one.
- **Whether `.serial`'s failure semantics are acceptable.** A failed create
  skips every test after it — including a delete meant to clean up, so the
  record it never got to create obviously can't leak, but a record created
  by test 1 and never reached by a failing test 3 does leak. There is no
  teardown fixture in this repo yet (R2), so this is the real trade-off, not
  a hypothetical one. Say so plainly and get an explicit yes.
- **Confirm the drift is recorded locally, not in ADO.** The edit will
  usually make the spec do something no ADO ticket describes. Say that up
  front: the amendment goes into the test case's own file (see Beat 4), and
  ADO itself is never touched.

## Beat 3 — Propose

State, and get explicit agreement on, before anything is written:

1. **A restatement of the intent** in your own words, so a misunderstanding
   surfaces here rather than in the diff.
2. **The blast radius** — the exact test titles that will be modified, added,
   removed, or unchained. This is the same shape `enforceRadius` checks
   mechanically later (`{ modify: string[], add: string[], remove: string[],
   unchain: string[] }`), and it is built by the same rules the dispatched
   agent must also follow:
   - **A reorder names every test whose position moved under `modify`**, even
     when no test body changes. `enforceRadius` checks this per test, not per
     file: it refuses each test whose position in the file moved and that
     isn't named, so declaring one test does not buy a reorder of the others.
     A change in running order is a change to the tests whose order changed,
     since once tests share state through a `describe`-scoped variable, order
     *is* behaviour.
   - **Chaining names every chained test under `modify`**, for the same
     reason and even though not one body changed. `test.describe.serial`
     rewrites the failure semantics of every test it wraps — one failure
     skips the rest — so wrapping three tests means naming all three.
     Unwrapping an existing chain is the same. The same rule covers a test
     moved between `describe` blocks, which changes its full test title and
     which hooks it inherits.
   - **The spec's `userKey` is never in the radius.** Changing the account a
     spec runs as is out of scope for this skill entirely and the gate
     refuses it unconditionally — no declaration can authorise it. If QA
     wants a different account, say so and stop; that is a deliberate edit
     of its own, with the side-switching consequences CLAUDE.md describes.
   - **A title change is `remove: [old title]` + `add: [new title]`, never
     `modify`.** Titles are the key the gate compares tests by. If the body
     also changes, it isn't a rename at all — it's a genuine removal plus a
     genuine addition, and it's named that way. You can always name a
     removed test's exact title here, because the human just pointed at it
     in beat 2.
   - **A title goes in `unchain` only when QA agreed at beat 3 to stop that
     test consuming the chain's shared record** — it stops reading an
     identifier an earlier test in the same `describe` created and starts
     sourcing or creating its own. Name it there in addition to `modify`,
     never instead of it: `unchain` tells the gate the chain break was
     agreed to, it doesn't tell the gate the body changed too.
3. **Before/after**, in plain English — what the spec looks like today,
   what it will look like after.
4. **The checks being given up**, by test and by count, whenever the edit
   drops any. Chaining is the usual case: a test that stops seeding its own
   record also stops asserting that the seed was created, because the test
   before it in the same chain already did. Say which tests lose how many
   checks and why each one is now redundant, and get an explicit yes. A drop
   on a test named under `modify` is *permitted* by the gate rather than
   refused, so beat 3 is the only place a human ever sees it — if you don't
   raise it here, nobody agreed to it. If nothing is dropped, say nothing.

Nothing is written yet. Once they agree, commit the checkpoint (see Beat 4)
and move on.

## Beat 4 — Apply

**Checkpoint first, before anything is touched.** Commit the spec, its page
object (if one exists), its data file, and its test case as a WIP checkpoint
— **a commit, never `git stash`**: the stash stack is shared across every
worktree of this repository, and a concurrent session can pop it out from
under you.

```bash
git add suites/<seg>/specs/<stem>.spec.ts suites/<seg>/pages/<feature>.page.ts \
        suites/<seg>/data/<feature>.json suites/<seg>/test-cases/<stem>.md
git commit -m "WIP checkpoint before /edit-spec: <one-line summary>"
git rev-parse HEAD
```

Report the SHA. Omit the page object from `git add` if the feature has none.

**Baseline, before dispatching.** Run the whole spec now, before the edit:

```bash
npx playwright test suites/<seg>/specs/<stem>.spec.ts --project=SL
```

Record which tests are green. **If any test is already red, tell them before
the edit starts and ask whether to proceed anyway.** A pre-existing failure
is never blamed on the edit and never quietly healed under cover of it —
that's what the classification table below exists to prevent.

**Dispatch `spec-editor`** with everything in its Input contract
(`.claude/agents/spec-editor.md`): the spec/test-case/data paths, `<seg>`,
QA's intent verbatim plus your beat 3 restatement, the declared blast radius
from beat 3, the baseline green set, the edit class (`restructure` or
`remove`), and the pointer to `docs/app-quirks.md`.

**Enforce the radius. `enforceRadius(before, after, beat3Radius)` is the
gate — the single authoritative check — and the beat 3 radius (what QA
actually agreed to) is the only permission it may ever be run against.**
`declared` means "the titles that may be touched," and the human at beat 3
is the only party entitled to grant that. The agent's `## Touched` report
(a fenced JSON block, `{ "modify": [...], "add": [...], "remove": [...],
"unchain": [...] }`, ending its report) is real information, but it is the
agent's own account of what it did — running `enforceRadius` against it
instead would be checking the agent's story against itself: a title touched
outside every permission anyone actually gave, but *honestly* named in the
report, would sail straight through. So `## Touched` is a **secondary honesty
cross-check only**: run `enforceRadius` a second time against it, and if it
disagrees with the authoritative result, that disagreement is a signal
about the agent's reliability to report to QA — it never overrides the
authoritative run and never applies anything by itself.

**Run it as a scratch script, never as a `node -e` one-liner.** Git-bash
rewrites an MSYS-style path (`/tmp/x`) when it is a *bare CLI argument*, but
it does **not** rewrite one embedded inside a `node -e` JavaScript source
string — `node` is a native Windows binary, so that unrewritten leading `/`
resolves as the current drive's root (`D:\tmp\x`) and the read fails with
`ENOENT`. Writing a small `.mjs` file sidesteps this entirely: its own
`import` paths resolve relative to the file's own location, never the
shell's, and its `fs` calls use a path relative to the working directory
you run `node` from — no path ever has to survive being embedded inside a
quoted `-e` string. Use a scratch directory **relative to the repo root**,
e.g. `.edit-spec-scratch/` (outside version control — nothing here is ever
committed):

```bash
mkdir -p .edit-spec-scratch
git show <sha>:suites/<seg>/specs/<stem>.spec.ts > .edit-spec-scratch/before.spec.ts
```

Save the `## Touched` block to a file, e.g. via the Write tool:

```
.edit-spec-scratch/declared.json  <-  the JSON *inside* the fences, verbatim
```

Verbatim means the object itself, character for character — the ```` ```json ````
and ```` ``` ```` lines are markdown, not JSON, and `gate.mjs` calls
`JSON.parse` on this file. Never re-key it, never re-order it, never fill in
a bucket the agent left empty: this file is the agent's own account of what
it did, and correcting it turns the honesty cross-check into a formality.

Then write `.edit-spec-scratch/gate.mjs`, filling in `<seg>`/`<stem>` and
the beat 3 radius you got QA's agreement on:

```js
import { parse } from '../cli/spec/model.mjs';
import { enforceRadius } from '../cli/spec/diff.mjs';
import fs from 'fs';

// What QA actually agreed to in beat 3 - the only valid permission.
const beat3Radius = { modify: [], add: [], remove: [], unchain: [] };

const before = parse('.edit-spec-scratch/before.spec.ts');
const after = parse('suites/<seg>/specs/<stem>.spec.ts');

const authoritative = enforceRadius(before, after, beat3Radius);
console.log('AUTHORITATIVE (beat-3 radius - the gate):');
console.log(JSON.stringify(authoritative, null, 2));

const touchedDeclared = JSON.parse(fs.readFileSync('.edit-spec-scratch/declared.json', 'utf8'));
const honesty = enforceRadius(before, after, touchedDeclared);
console.log("\nHONESTY CHECK (spec-editor's own Touched report - informational only):");
console.log(JSON.stringify(honesty, null, 2));
```

Run it from the repo root:

```bash
node .edit-spec-scratch/gate.mjs
```

Act only on the `AUTHORITATIVE` result. If it disagrees with the honesty
check (e.g. the honesty check passes while the authoritative run doesn't —
the agent touched something within what it reported but outside what QA
approved), say so to QA explicitly as a reliability signal about this
`spec-editor` run, separate from the refusal itself.

**Any violation in the authoritative result: restore the checkpoint and
report. Nothing half-applied.**

```bash
git checkout <sha> -- suites/<seg>/specs/<stem>.spec.ts suites/<seg>/pages/<feature>.page.ts \
                      suites/<seg>/data/<feature>.json suites/<seg>/test-cases/<stem>.md
```

**`git checkout <sha> -- <path>` restores files that existed at the
checkpoint; it cannot delete a file that didn't.** If the edit *created* a
page object the feature never had, that file is untracked and survives the
revert untouched — so the tree is not byte-identical to before, and the
"nothing half-applied" claim is false until you remove it. Check for that
before claiming a clean revert:

```bash
git status --porcelain suites/<seg>/
```

Anything still listed — an untracked `??` page object is the realistic case
— must be deleted by hand. Then re-run `git status --porcelain suites/<seg>/`
and confirm it prints nothing before reporting **Reverted**.

Then delete the scratch directory — `rm -rf .edit-spec-scratch` — whether
the gate passed or failed; nothing in it is ever meant to persist past this
beat. It is gitignored, so a stray `git add -A` in between won't sweep the
copied spec into a commit, but the cleanup is not optional for that reason.

**Explain a refusal in plain terms — never dump the raw violation list.**
`enforceRadius`'s messages are precise but written for the gate's own
maintainer, not for QA:

| The violation says | What actually happened | What to tell them |
|---|---|---|
| `"X" was modified but is outside the declared radius` | The edit touched a test QA never approved in beat 3 | "The edit changed `X`, which wasn't part of what we agreed — nothing was applied." |
| `"X" was removed but removal was not declared` / `"X" was added but addition was not declared` | A test disappeared or appeared that wasn't named in the beat 3 radius | Same shape — name the test, say it wasn't agreed, say nothing was applied. |
| `"X" was renamed to "Y" but the rename was not declared` | A rename landed under `modify` in the radius instead of `remove`+`add` | "That's a rename that wasn't declared correctly — I'm re-checking rather than applying." |
| `"X" was reordered but is outside the declared radius` | A test's position moved and it wasn't named — the message lists every title that moved | "The edit changed the order `X` runs in, which wasn't part of what we agreed — nothing was applied." |
| `"X" was chained into test.describe.serial` / `was unchained from` | A test was wrapped into (or out of) a chain without being named. Its body may be untouched — chaining changes how it *runs*, not what it does | "`X` now only runs if the test before it passes (or: no longer runs in a chain) — that changes its behaviour and wasn't agreed." |
| `"X" moved from describe block ... to ...` | A test changed `describe` membership, which changes its full title and the hooks it inherits | "`X` was moved into a different group, which renames it as far as the test runner is concerned — that wasn't agreed." |
| `"X" stopped reading \`name\` from its describe block ...` | The test no longer reads a variable the chain's earlier test created for it — it's unchained from the shared record, and every other check still passes, so this is the only signal | "`X` no longer uses the record the chain shares, so it's no longer really part of the chain — if that was the plan, it needs declaring under `unchain`." |
| `the spec's userKey changed from ... to ...` | The account the whole spec runs as was swapped | "The edit changed which login this spec runs as. That's out of scope for this skill entirely and nothing can authorise it — nothing was applied." |
| `duplicate test title(s) ... cannot be addressed by title` | Two tests already share a title, so nothing can be safely compared | "There are two tests both called `X` — that has to be resolved by hand before this skill can touch either one." |
| `"X" lost N assertion(s)` | A check disappeared from a test **not** named under `modify` | "The edit weakened what `X` checks, and `X` wasn't part of what we agreed — nothing was applied." A drop on a *declared* test never appears here; it is permitted, and comes back as a permitted drop instead (below). |
| `the edit's assertion budget doesn't add up` | The file's total fell further than the declared removals and permitted drops account for — or the edit added a test, which forfeits all credit | "Taken together, this edit checks less than before without saying so — something's missing." |

**Report the permitted drops before moving on.** A passing gate is not the
same as an unchanged one. The `AUTHORITATIVE` block the gate script printed
carries a `diff.droppedAssertions` array, and it lists every test
that came out with fewer checks than it went in with — each one permitted
*only* because QA named it under `modify` at beat 3. Print them back in plain
terms:

> This edit gave up 6 checks in `User can edit an existing rating method` and
> 4 in `User can delete an existing rating method` — the seed-creation
> assertions the chained create test now covers. You agreed to this at beat 3.

Cross-check that list against the agent's own `## Assertions given up`
section. A drop the gate saw that the agent did not list is a reliability
signal about this `spec-editor` run, reported to QA the same way a `## Touched`
disagreement is — it is not itself a refusal.

**Once the radius holds, run the data commands the agent returned** — each
`npm run data:edit -- ... --apply` verb it listed (`remove-case`,
`remove-field`, `rename-case` are the ones a restructure or removal reaches
for; check `cli/data/edit.mjs` for exact argument order if a command looks
off rather than guessing).

**Run the spec again and compare green sets.**

```bash
npx playwright test suites/<seg>/specs/<stem>.spec.ts --project=SL
```

Classify every red test — **never lump them together**:

| What is red | Action |
|---|---|
| Red before **and** after | Report, leave alone. Not the edit's problem; not repaired under cover of an edit. |
| Green before, red after, **outside** the radius | Bounded healer — the `/gen-test` step 4 bounds block, verbatim (below). |
| Red **inside** the radius | **Stop. Ask QA. No healer.** This is where intent gets erased. |
| Passing, but the app visibly misbehaves | Report as a product bug. Never heal into green. |

The bounded-healer dispatch for the second row carries this verbatim, on top
of whatever else the dispatch says:

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

**Lint — non-optional.**

```bash
npm run lint:tests -- --require-specs
```

Must pass, and the `Scanned N spec file(s).` line must include this spec.
`assertion-count` is the only thing that catches a quietly weakened
`expect(` after the fact — a weakened assertion still passes, so the
before/after green-set comparison alone is blind to it.

**Re-baseline the assertion floor — only if lint failed on `assertion-count`,
and only once everything else is green.**

A restructure that removed redundant ceremony leaves the spec below the sum of
its raw counterparts, and `assertion-count` compares against that sum by
default. That sum is the right floor for a raw→refactored transformation and
the wrong one for a spec that no longer has the independent structure it was
measured against. Look at the numbers first:

```bash
npm run spec:floor -- <stem>
```

Then record the new floor, reusing the words you put in the amendment:

```bash
npm run spec:floor -- <stem> --apply --reason "chained 5 tests into test.describe.serial around one shared record"
```

It measures the spec with the linter's own counter and appends an
`**Assertion floor:** <count> (raw sum <count>)` line to the test case.
**Never write, edit, or adjust that line by hand, and never choose the number
yourself** — same trust model as a `_provenance` marker. Then re-run:

```bash
npm run lint:tests -- --require-specs
```

and it must pass before you report Applied.

If `assertion-count` was *not* what failed, do not run `spec:floor` at all. A
floor is not a way past any other rule, and `--apply` refuses on a spec that
already meets its raw floor.

**Write the dated drift amendment** into `suites/<seg>/test-cases/<stem>.md`
— append, never edit, an `## <id>` section:

```markdown
## Amendment — <today's date>

QA asked: "<their words, verbatim>"

Changed: <plain description of what the spec now does that it didn't before>

Why: <the answer from beat 2's questions>

No ADO ticket describes this change; ADO is not updated — the drift is
recorded here only.

<!-- `npm run spec:floor -- <stem> --apply` appends its own
     **Assertion floor:** line below this section when the restructure left the
     spec under its raw-counterpart sum. Never write that line by hand. -->
```

## The insight loop, bounded at 3 rounds

When a test inside the radius is red (row three of the classification
table), do not dispatch a healer. Instead:

1. Report to QA what was tried, exactly which step failed, and what was
   observed.
2. Ask the question only a human can answer — e.g. *"the delete failed
   because the row has no Delete control visible — is delete somewhere
   else, or does this screen not support it?"*
3. Feed their answer into a **fresh** `spec-editor` dispatch as an explicit
   diagnosis to act on, carrying the prior report so it doesn't repeat
   itself.

**Ceiling: 3 rounds.** After the third round without a green result inside
the radius, stop offering to retry and offer to revert instead.

## Three terminal states — always name one

- **Applied** — edit in, spec green, no regressions outside what was
  declared, lint clean. Report the before/after green sets and the lint line
  together; never report "green" without both.
- **Reverted** — checkpoint restored with `git checkout <sha> -- <the four
  files>`, tree back to exactly where it started. Nothing half-applied.
- **Parked** — the edit is left in place, the failing test is `fixme`'d with
  a one-line reason, and this is reported loudly. Only ever with QA's
  explicit say-so — never a default when the insight loop runs out.

## Beat 5 — Render again

```bash
npm run spec:show -- <stem>
```

Print it verbatim, the same way beat 1 did. A passing test proves the code
runs, not that it does what QA asked for — only this closing render, read by
the human who opened the request, proves intent was actually achieved.

## Out of scope in R1 — say so and stop

- **Cleanup / teardown.** There is no `afterEach`/`afterAll`/teardown fixture
  anywhere in this repository yet. If QA wants a test to clean up after
  itself beyond what `.serial` chaining gives them, say that plainly (see
  beat 2's `.serial` question above) rather than improvising a fixture.
- **Changing what a test does** — new steps, new assertions, new values.
  This needs live DOM verification, and the zero-hallucination rule admits
  no exception for an edit. Not built yet.
- **Adding a new ADO case to a spec.** That's `/gen-test <id> --spec <stem>`,
  not this skill — it appends, this skill reshapes what's already there.

## Never

- Never hand-edit a spec, page object, data file, or test case yourself.
  Every content change goes through the dispatched `spec-editor`; every data
  mutation goes through `npm run data:edit`; the drift amendment is the one
  thing the skill itself writes, and only as an append.
- Never delegate this skill to a subagent. It is main-session only, always.
- Never skip the baseline run, and never blame a pre-existing failure on the
  edit.
- Never dispatch a healer for a test inside the declared radius. That's the
  insight loop, not the healer.
- Never use `git stash` for the checkpoint. A commit, always.
- Never report "applied" without the lint line and both green sets beside
  it.
- Never write or adjust an `**Assertion floor:**` line by hand, and never
  choose the number. `npm run spec:floor -- <stem> --apply` measures it.
- Never treat a violation from either radius check as partial permission —
  any violation means restore and report, not "apply the parts that were
  fine."

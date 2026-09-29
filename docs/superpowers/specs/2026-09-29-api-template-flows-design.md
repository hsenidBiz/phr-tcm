# API Template flows: the application's stage order, enforced

Design, agreed with the owner on 2026-09-29. Builds on
`2026-09-28-api-templates-design.md` (called "the templates spec" below).

## 1. Why

The templates spec found that **step order is not enforced server side**:
each wizard handler accepts any `cycleId`, and only Publish checks that the
required steps are complete (templates spec §3). So an assistant can prove
*Create a draft performance cycle*, then prove *Add participants* against
the new cycle straight away - skipping evaluation rules, timeline and
evaluators - and get a cycle no person using the UI could ever produce.
Test data in an impossible state tests nothing.

This design gives each module's wizard a **flow**: its stages, what must
come before what, which stages may be skipped, and a database check for
whether a stage is done for a given record. The app **refuses** a template
whose required earlier stages are not done for that record, and tells the
assistant, step by step, what comes next. The API Templates tab draws each
flow as a **map**.

Success means: with the performance cycle flow saved, proving or running a
participants template against a cycle whose evaluation rules are not saved
is refused before any request, with a sentence naming the stage to do next
and the template that does it; and the tab shows the cycle wizard as a
map, stage by stage, with the templates on each stage and the gaps.

## 2. Owner decisions

1. A run or prove whose required earlier stages are not done is
   **refused**. There is no override.
2. Whether a stage is done is **asked of the application's database**, by
   a read-only check on each stage, run by the app on the database chosen
   on the AI Bridge tab. Not the app's own run history: the database also
   sees cycles made by hand, and saves that un-complete later steps
   (templates spec §3).
3. **A stage marked optional may always be skipped**, and so no stage may
   require it. Every other stage must be done before any stage that
   requires it.
4. **Proving is gated exactly like running** - a proving run writes real
   data too. And a proving run must leave its own stage done: after every
   step passes, the template's stage check must be true for the record, or
   nothing is saved.
5. The tab shows a **flow map** per module: stages in order, "requires"
   arrows, optional stages dashed, the templates on each stage, and the
   stages no template performs yet. A record's live progress is not shown
   in the tab (§11).
6. Everything here is offered **exactly where Auto Run is**, like the rest
   of API templates, and is never named in the changelog or How To Use.

## 3. Flow format

One JSON file per flow, beside the templates:
`<Auto Run data>/flows/<org-project slug>/<flow-id>.json`. Written only by
`save_api_flow` (§6); never edited in place.

```jsonc
{
  "id": "pms-performance-cycle",
  "title": "Performance cycle wizard",
  "module": "PMS / Performance Cycle",
  "subject": { "name": "cycleId", "type": "number" },
  "sources": ["Pages/PerformanceCycle/Index.cshtml.cs:40"],
  "stages": [
    { "id": "setup", "title": "Cycle setup", "creates": true,
      "check": "SELECT 1 FROM PeoplesHR.perf_cycle WHERE cycle_id = {{cycleId}}" },
    { "id": "rules", "title": "Evaluation rules", "requires": ["setup"],
      "check": "SELECT 1 FROM PeoplesHR.perf_cycle_step_progress WHERE cycle_id = {{cycleId}} AND step_key = 'EvalRules' AND is_complete = 1" },
    { "id": "competencies", "title": "Competencies", "requires": ["rules"], "optional": true,
      "check": "SELECT 1 FROM ... WHERE cycle_id = {{cycleId}} AND ..." },
    { "id": "participants", "title": "Participants", "requires": ["rules"],
      "check": "SELECT 1 FROM ... WHERE cycle_id = {{cycleId}}" },
    { "id": "publish", "title": "Publish", "requires": ["participants"],
      "check": "SELECT 1 FROM PeoplesHR.perf_cycle WHERE cycle_id = {{cycleId}} AND status = 'Published'" }
  ],
  "saved": { "at": "2026-09-29T09:00:00Z", "sample": 273 }
}
```

(Table and column names above are illustrative; the assistant reads the
real ones from the code and the database.) In that example Competencies is
optional; Publish is required by nothing too, but it is not optional - it
is the flow's last stage, and nothing is skipped by leaving it out.

Rules - all reported together, as the templates spec's `check` does:

- **Unknown fields are refused** at every level.
- **`id`**: the template `id` rule (`api_templates::valid_id`). Stage ids
  follow it too and are unique within the flow.
- **`subject`**: `name` is a placeholder name (letters, digits, `_`);
  `type` is `number` or `string` - nothing else can identify a record.
- **Exactly one stage `creates`** the record. It has no `requires`, and its
  check is the "record exists" check. Every other stage has at least one
  `requires`.
- **`requires`** names stages of this flow, never itself; the stages and
  their `requires` must not loop back on themselves (no cycles).
- **Every stage is reachable** from the creating stage through `requires`.
- **`optional`** (default `false`) marks a stage that may be skipped.
  **No stage may require an optional stage** - that is what makes skipping
  it safe (decision 3). An optional stage may itself require others; it
  still cannot run before them.
- **`check`** is one statement containing `{{<subject name>}}` at least
  once and no other placeholder. After the subject is substituted (below)
  it must classify as a read (`db::guard::classify` -> `Verdict::Read`).
  The stage counts as **done** when the statement returns at least one
  row.
- **Substitution into a check is typed, never textual.** A `number`
  subject must be a JSON integer and is written as its digits; a `string`
  subject is written as an `N'...'` literal with every `'` doubled. A
  value of the wrong type is refused before any statement runs. The
  substituted statement is classified again, so the guard sees exactly
  what runs.
- **At most 30 stages**; a check at most `guard::MAX_SQL_CHARS`.
- **`saved`** is written by the app (§6); a flow sent with one is refused.

## 4. Templates join a flow

`ApiTemplate` gains one optional field:

```jsonc
"stage": { "flow": "pms-performance-cycle", "id": "participants" }
```

Checked when a template is proven (not when an old template is read back,
so templates saved before this change keep loading):

- The flow must be saved for this project, and name that stage.
- **The creating stage's template** must `capture` the subject name and
  list it in `outputs` - that is how the record id comes back.
- **Any other stage's template** must declare a parameter named after the
  subject, of the subject's type (`number` or `string`).
- A template with no `stage` is not in any flow and runs exactly as today.
  The guide (§7) tells the assistant that every template acting on a
  flow's record must name its stage.

## 5. The gate

Applies to `prove_api_template` and `run_api_template` alike, for a
template with a `stage`. It runs after the templates spec's own checks
(§5 step 1 there) and **before sign-in** - a refused run opens no browser.

1. **Load** the flow and find the stage. Missing flow or stage: refused -
   `this template's flow pms-performance-cycle is no longer saved` /
   `stage "participants" is no longer in flow pms-performance-cycle`.
2. **Database.** The connection is the one chosen on the AI Bridge tab
   (`BridgeContext::db_id`, resolved the way `db_query` resolves it). None
   chosen: refused - `this template belongs to a flow, and flow checks
   need a database: choose one on the AI Bridge tab`.
3. **Which stages.** The creating stage's template skips the gate (there is
   no record yet). For any other stage: every stage in the transitive
   closure of its `requires`. By construction (§3) no optional stage is
   ever in it.
4. **Checks.** Each is substituted with the value supplied for the subject
   and run as a read on that connection, through the same `sqlcmd` read
   path `db_query` uses, 15 seconds each. They run in flow order and all
   of them run, so the answer is complete.
5. **Refusal**, if any is not done, names the **earliest** missing stages
   (those not done whose own `requires` are all done) and, for each, the
   saved templates that perform it:
   `Evaluation rules is not done for cycleId 274 - do it first with
   pms-set-eval-rules (Set the evaluation rules). Then: Participants.`
   A stage with no template yet says so: `... no template performs
   Evaluation rules yet: prove one first`.
   A check that fails to run (a database error, a timeout) refuses the run
   naming the stage and saying its check could not be run - never treated
   as "not done". The database error goes to the activity log, not the
   answer, under the same rule as every other user-facing error.
6. **After a successful prove** (every step passed), the template's **own**
   stage check runs with the subject - from the values, or, for the
   creating stage, from the capture. False: the template is **not saved**,
   and the answer says the steps ran but did not complete the stage, with
   what was created (templates spec §5, "No rollback"). A run does not do
   this second check: a proven template has already shown it completes its
   stage.

Every check is recorded in the activity log as a database lookup (the
statement in full, `ok`, rows, duration), with the template id and the
stage it was for.

## 6. Saving and removing a flow

`save_api_flow { flow, sample, replace?, why? }`:

- Everything in §3 is checked; all problems in one answer.
- **`sample`** is the subject of a real record (the assistant finds one
  with `db_query`). Every stage's check is run once with it, on the chosen
  database; each must *run* - true or false are both fine. A check that
  fails to run refuses the save, naming the stage. The answer returns each
  stage's result for the sample, so the assistant sees the flow working.
- Replacing an existing id needs `replace: true` and a `why` (logged), as
  templates do.
- A replacement that drops or renames a stage some saved template performs
  is **saved**, and the answer lists those templates: they are refused on
  their next run (§5 step 1) until the assistant proves a replacement.
- No API templates switch is needed: saving a flow writes one local file
  and reads the database; it writes nothing to the application.
- Emits the existing `ApiTemplatesChanged` event.

**Removing** is the user's, in the tab (§8), like templates. There is no
tool that removes a flow.

## 7. Assistant tools and guide

Two new MCP tools, in `DEV_ONLY_TOOLS` on both sides (Rust and
`src/lib/mcpTools.ts`) so they are offered exactly where the other API
template tools are, and part of the AI Bridge tab's existing
"API templates" row:

| Tool | Route | Does |
|---|---|---|
| `save_api_flow` | `POST /api-template-flow-save` | §6. |
| `get_api_flow_progress` | `POST /api-template-flow-progress` | `{ flow, subject }` - runs every check for that record and returns each stage as `done`, `next` (not done, every `requires` done), `blocked` (some `requires` not done) or `skippable` (optional and not done), with the templates on each stage. The step-by-step direction. |

Both routes start with `/api-template`, so the existing path guard
(`autorun_guard_for`) covers them without a new line. Neither needs the API
templates switch - both only read.

Changes to the existing tools:

- `list_api_templates` also returns the project's flows (id, title, module,
  subject, stages with `requires`, optional, and the templates on each) and
  each template's `stage`.
- `get_api_template_guide` gains a **Flows** section: the format (§3), the
  gate (§5), and the order of work - *map the wizard first* (read the
  page's steps and where their progress is stored, write a check per
  stage, save the flow with a sample), *then* build templates stage by
  stage from the creating stage, and *before every run*, call
  `get_api_flow_progress` and run a `next` stage's template.
- `prove_api_template` and `run_api_template` keep their inputs; refusals
  from §5 come back like any other problem.

## 8. The tab

Above the template list, one **flow map** per flow, grouped with its
module:

- **Header:** flow title, module, the subject name (`cycleId`), when it
  was saved, and **Remove flow** (a confirmation naming the flow and how
  many templates perform its stages; those templates stay, and are refused
  until a flow with their stage is saved again).
- **Map:** stages laid out left to right by depth - a stage's column is
  the length of the longest `requires` path from the creating stage; within
  a column, declaration order. Arrows from each required stage to the stage
  that requires it. Each stage is a box: title, then the templates that
  perform it (title and effect badge, as in the list); a stage with no
  template shows **No template yet** in the `faint` token. Optional stages
  have a dashed border and an "Optional" label. Clicking a template
  expands and scrolls to its row below.
- A map wider than the tab scrolls horizontally inside its own box; the
  page never scrolls sideways.
- Drawn by the app itself - SVG for the arrows, ordinary elements for the
  boxes. No graph library (the app has none, and a layered layout of at
  most 30 stages does not need one).
- **Accessibility:** the SVG is `aria-hidden`; beside it, a visually
  hidden ordered list gives the same information as text (stage, what it
  requires, optional, its templates).
- **Template rows** gain a "Stage: Evaluation rules (Performance cycle
  wizard)" line; a template whose flow or stage is no longer saved says so
  in the `warning` token.
- The tab's search also matches stage titles.
- Colours from tokens only; `src/ui-consistency.test.ts` and the a11y
  suite pass unchanged.

The tab reads everything through the existing `api_templates_overview`
command, which gains `flows`. Removing a flow is a new command beside
`api_templates_remove`, gated by the same `refuse_unless_offered`.

## 9. Errors

Additions to the templates spec's §9, all in one answer where they occur
before any request:

- a malformed flow (every rule in §3);
- a template's `stage` naming a missing flow or stage, or the wrong
  subject parameter or capture;
- no database chosen; a check that could not run; a check that ran and
  found a required stage not done (§5 step 5);
- a proving run that passed every step but did not complete its stage.

No answer ever includes a raw database or transport error.

## 10. Testing

Rust, in the existing suite modules (`suite/api_templates.rs` for the
model, `suite/api_templates_runner.rs` for the gate, `suite/ai_bridge.rs`
and `suite/tcm_mcp.rs` for the routes and tools):

- Flow checks, one test per rule in §3, including a cycle, a stage
  requiring itself, two creating stages, none, and a stage requiring an
  optional one.
- Typed substitution: an integer is written as digits; a string becomes
  `N'...'` with quotes doubled; `1; DROP TABLE x`, `1 OR 1=1` as a number,
  and a non-integer are refused; the substituted statement is classified
  again.
- Required set: the transitive closure; no optional stage in it; the
  creating stage's template skips the gate.
- The gate against a fake `Runner` (the one `db_query`'s tests use): all
  done -> the run proceeds; one missing -> refused naming the earliest
  missing stage and its template; a stage with no template; a check that
  errors -> refused as "could not be run", not "not done"; no database
  chosen; flow or stage gone.
- The gate runs before sign-in: a refused run never touches the `Driver`.
- Prove: steps pass but the own-stage check is false -> nothing saved;
  the creating stage's check reads the captured subject.
- `save_api_flow`: sample run for every stage; a failing check refuses;
  `replace` + `why`; orphaned templates listed.
- `get_api_flow_progress`: done / next / blocked / skippable.
- Gating: the two tools absent while locked and refused by call; both
  routes refused by the path guard while locked; the remove-flow command
  refused while locked.
- Activity log: each check recorded as a lookup with its template and
  stage; the app log's summary line carries no SQL.
- `tests/bindings.rs` regenerated; nothing token-shaped.

Frontend (vitest):

- The map: column by depth, arrows present for each `requires`, dashed
  optional stage, "No template yet", clicking a template expands its row,
  the text list equivalent, horizontal scroll inside its own box.
- Remove flow confirmation; template row's stage line and the "no longer
  saved" warning; search matches stage titles.
- `mcpTools.ts`'s `DEV_ONLY_TOOLS` in sync with Rust (the existing sync
  test).
- `docs-site/src/guard.test.ts`'s term list gains `save_api_flow` and
  `get_api_flow_progress`.

Acceptance: the assistant saves the performance cycle flow against dev01
with a sample cycle; `get_api_flow_progress` on a new draft cycle shows
Cycle setup done and Evaluation rules next; running a participants
template on that cycle is refused naming Evaluation rules; after the
evaluation rules template runs, the participants template runs.

## 11. Not in this change

- A record's live progress in the tab (enter a `cycleId`, see stages
  coloured). The assistant has it through `get_api_flow_progress`.
- Overriding the gate.
- Flows that span modules, or more than one record (a subject per flow).
- The app choosing or chaining templates itself; the assistant still picks
  each run.
- Stage checks from the app's own run history.

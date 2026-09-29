//! What `get_api_template_guide` hands the assistant - design doc "API
//! templates" §4 (the format) and §7 (the tool). The format and the
//! authoring workflow are fixed text; the project's account KEYS and the
//! sign-in recipe's origin are added at the end, so the assistant never
//! guesses an account or a host.
//!
//! Only keys ever reach this module: `text` takes the keys, not the
//! accounts, so a username or a password has no way into the guide.

const BASE: &str = r#"# API templates

An API template writes test data through the application's OWN endpoints -
the same requests its pages send - from inside a browser that signed in the
way a person does. Use one when a test needs data set up (a cycle created,
a record moved to a state) and clicking through the screens would be slow.

A template is saved ONLY by a proving run in which every step passed. There
is no tool that sends a free-form request, and none that removes a
template: removing one is the person's, on the API Templates tab.

## The format

One JSON object:

    {
      "id": "pms-create-draft-cycle",
      "title": "Create a draft performance cycle",
      "module": "PMS / Performance Cycle",
      "effect": "create",
      "description": "Cycle setup + evaluation rules; leaves the cycle in Draft.",
      "sources": ["Pages/PerformanceCycle/Index.CycleSetup.cshtml.cs:95"],
      "antiforgery": { "page": "/hr/pmsv10/performancecycle?mode=create" },
      "params": [
        { "name": "cycleName", "type": "string", "required": true,
          "description": "Shown in the cycle list" },
        { "name": "ratingMethodId", "type": "number", "required": true,
          "lookup": "SELECT Id FROM ... WHERE Name = @ratingMethod" }
      ],
      "steps": [
        { "name": "Cycle setup", "method": "POST",
          "path": "/hr/pmsv10/performancecycle", "query": { "handler": "SaveProgress" },
          "form": { "CycleName": "{{cycleName}}" },
          "expect": { "status": 200, "json": { "success": true } },
          "capture": { "cycleId": "$.cycleId" } },
        { "name": "Evaluation rules", "method": "POST",
          "path": "/hr/pmsv10/performancecycle", "query": { "handler": "SaveEvalRulesProgress" },
          "form": { "CycleId": "{{cycleId}}", "RatingMethodId": "{{ratingMethodId}}" },
          "expect": { "status": 200, "json": { "success": true } } }
      ],
      "outputs": ["cycleId"]
    }

Rules - a draft that breaks any of them is refused before anything runs,
with every problem listed together:

- Unknown fields are refused, at every level.
- `id`: lowercase ASCII letters, digits, `-` and `_`; 1 to 100 characters.
  It is the file name.
- `effect`: `create`, `edit` or `delete` - what the template does to the
  application's data. You declare it; the tab shows it as a badge.
- `sources`: where in the application's code each request comes from, as
  `file:line`.
- `params[].type`: `string`, `number`, `boolean`, `date` (`YYYY-MM-DD`) or
  `list` (a JSON array). `lookup` records the query that finds a value; the
  app never runs it - it is there for the next person, or you, to repeat.
- `method`: `GET` or `POST` only. A handler that deletes is still a POST.
- `path`: relative, starting with a single `/` - never a host, never `//`,
  a backslash, a `..` segment, a `?` or a `#` (query parameters go in
  `query`). Every request goes to the origin of this
  project's sign-in recipe, named at the end of this guide. `query` values
  are URL-encoded by the app.
- Write every `path`, and `antiforgery.page`, in exactly the letter case
  the application's own UI requests it - read it from the UI's network
  calls or the app's routing, not from a class or file name. Cookie paths
  are case-sensitive: an application that keeps its anti-forgery cookie on
  `/hr/pmsv10` never receives it at `/hr/PMSV10/...`, and a save without
  it comes back an empty 400. If a path still differs from a cookie's
  path only in letter case, the runner sends it in the cookie's case (the
  application's routing does not mind) and says so in the activity log.
- The account: PeoplesHR lets an account be signed in in
  one place at a time. When the runner has to sign in afresh, the recipe
  clicks "Continue here", which logs that account out wherever else it is signed in - and
  anyone who signs in as it while a template runs ends the run's session,
  which shows as an empty 400 partway through. Run templates as an account
  nobody is using by hand.
- An empty 400 means the application refused a request before reading it
  (a busy moment on a shared server does it too), so nothing was saved:
  the runner sends that step once more with a fresh token before failing
  it. A step that fails "refused the same way on a second try" needs a
  quiet moment or the account's other session closed - not a changed
  template.
- A step has at most one body: `json` (any JSON value) or `form` (string
  values, sent as multipart/form-data - what the application's own forms
  send). File fields are not supported.
- `expect` defaults to `{ "status": 200 }`. Its `json` is a partial match:
  every key given must be present in the response with an equal value.
- `outputs`: captured names handed back to you when a run succeeds.
- `antiforgery.page`: a page that renders a form. It is opened once per run
  and its `__RequestVerificationToken` is sent with every step.
- `proven` is written by the app from the proving run. Never send one.

## Placeholders

`{{name}}` names a declared param or a value captured by an EARLIER step. A
placeholder that is the whole JSON value keeps the value's type (a number
stays a number, a list stays a list); inside a longer string it is text. A
placeholder naming nothing declared-or-earlier is refused when the draft is
checked, not halfway through a run.

## Captures

`capture` maps a name to a path in the step's JSON response: `$.a.b`,
`$.list[0]`, or `$.list[*].id` for an array of every element's `id`. A
capture that finds nothing fails the step.

## How to build one

1. Call `list_api_templates`: the template you need may already exist.
2. Read the module's code - the page's handler (`OnPostSaveProgress` and
   the like) gives the path, the `handler` query value, the form field names
   and the JSON it answers with; the page that renders the form is the
   `antiforgery` page. Record each request's location in `sources`.
3. Find real values with `db_query` (and `db_lookup` for the tables): an id
   the form expects, a name that must be unique. Put the query you used in
   the param's `lookup`. Ids differ between environments, so look them up
   against the database of the site the recipe signs into, every time.
4. Call `prove_api_template` with `{ template, account, values }`. It runs
   the draft; only if every step passes is the template saved.
5. Afterwards, `run_api_template` with `{ id, account, values }` returns the
   outputs.

`prove_api_template` and `run_api_template` are refused while the API
templates switch on the AI Bridge tab is off - ask the person to turn it on.
The guide and the list answer either way.

Proving an id that is already saved replaces it only with `replace: true`
and a `why` (the reason is logged). Both calls take an optional `browser`:
`"edge"` (the default) or `"chrome"`. One template runs at a time.

## Flows

A wizard (a cycle set up over several pages) is done in an order the
application enforces. A FLOW maps that order: its stages, and for each a
database check that says whether the stage is done for one record. Templates
then name the stage they perform, and the app refuses a template whose
earlier stages are not done for the record.

One JSON object, saved with `save_api_flow`:

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
      ]
    }

(Table and column names above are illustrative - read the real ones from the
code and the database.) Rules - a flow that breaks any of them is refused
with every problem listed together:

- Unknown fields are refused, at every level. `saved` is written by the app;
  never send one.
- `id` and each stage `id` follow the template `id` rule; stage ids are
  unique in the flow. At most 30 stages.
- `subject`: the name of the value that identifies the record (letters,
  digits, `_`) and its type, `number` or `string`.
- Exactly one stage has `creates: true`. It has no `requires`; its check is
  "the record exists". Every other stage has at least one `requires`, naming
  stages of this flow - never itself, never in a loop, and every stage must
  be reachable from the creating one.
- `optional: true` means the stage MAY BE SKIPPED. No stage may require an
  optional stage - that is what makes skipping it safe. Do not mark a stage
  optional unless the application really lets the wizard go on without it.
- `check`: one read statement containing `{{<subject name>}}` at least once
  and no other placeholder. The stage is done when it returns at least one
  row. The subject is put in by the app by its type, never as text: a
  number must be a whole number, a string is quoted for you. Do not put
  `SET NOCOUNT ON;` in a check - the answer needs its row count.
- Write each check to return a row only when the stage is done.
  `SELECT COUNT(*) ...` and `SELECT CASE WHEN EXISTS ...` always return one
  row, so such a check would always read as done: filter with `WHERE`
  instead, e.g. `SELECT 1 FROM ... WHERE cycle_id = {{cycleId}} AND ...`.
- A template that acts on a flow's record names its stage:
  `"stage": { "flow": "pms-performance-cycle", "id": "participants" }`. The
  creating stage's template must `capture` the subject name and list it in
  `outputs`; every other stage's template must declare a param named after
  the subject, of its type. A `number` subject must be captured as a JSON
  number (`274`, not `"274"`); a capture of the wrong type is not saved.

The gate: a template on a flow is refused, before anything runs, until its
`requires` stages are done for the record. The refusal names the stage that
is not done and the template that performs it. A stage with no saved
template yet says to prove one first.

A prove of a template on a flow is saved only if its own stage's check reads
done afterwards - for the creating stage, on the subject it captured. A
check that could not run saves nothing either, and says so.

The order of work - flows first, templates second:

1. Map the wizard first. Read the page's steps in the code and find where
   each step's progress is stored (a progress table, a status column, the
   rows the step writes). Write one check per stage.
2. Find a real record with `db_query` and call `save_api_flow` with
   `{ flow, sample }`, the sample being that record's subject. The answer
   gives every stage's result for the sample, so a check that is wrong
   shows at once: fix it and save again (`replace: true` and a `why` for an
   id already saved).
3. Then build the templates stage by stage, starting from the creating
   stage (no record exists before it), each proven with `stage` set.
4. Before EVERY run of a template on a flow, call `get_api_flow_progress`
   with `{ flow, subject }` and run the template of a stage marked `next`.
   A stage is `done`, `next` (not done, and every stage it requires is
   done), `blocked` (a stage it requires is not done), `skippable` (optional
   and not done) or `could_not_check` (its check could not run - fix the
   check or the database choice, never assume). Run an optional stage's
   template only if the test needs it.

`save_api_flow` and `get_api_flow_progress` need a database chosen on the AI
Bridge tab; neither needs the API templates switch. Proving or running a
template on a flow needs a database chosen on the AI Bridge tab too - only a
run of the creating stage's template checks nothing. Every flow check reads
the company database, so while the person has switched off Company database
(read), each call that would run one is refused until it is switched on.

## When a run fails

The first failing step stops the run. Nothing is rolled back - the
application has no undo - so the answer lists what had already been
created, which step failed with its handler, and the start of the
response. A failed prove saves nothing; fix the draft and prove again,
remembering that whatever it created is still there.

Limits: 30 seconds per step, 3 minutes per run, the first 64 KB of each
response.
"#;

/// The guide: `BASE`, then this project's account keys and the recipe's
/// origin - or, for either that is missing, what the person has to set up.
pub fn text(accounts: &[String], origin: Option<&str>) -> String {
    let mut out = BASE.to_string();
    out.push_str("\n## This project\n\n");
    if accounts.is_empty() {
        out.push_str(
            "No Auto Run accounts are saved yet - the person adds them on the Auto Run tab. \
             Nothing can be proven or run until one exists.\n",
        );
    } else {
        let keys: Vec<String> = accounts.iter().map(|k| format!("\"{k}\"")).collect();
        out.push_str(&format!(
            "Accounts - send one of these keys as `account`, never a username or a password: {}.\n",
            keys.join(", ")
        ));
    }
    match origin {
        Some(o) => out.push_str(&format!(
            "Templates run against {o}. Steps hold paths only - never put this host in a template.\n"
        )),
        None => out.push_str(
            "This project has no sign-in recipe yet - the person sets one up on the Auto Run tab. \
             Nothing can be proven or run until it has one.\n",
        ),
    }
    out
}

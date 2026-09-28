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
   the param's `lookup`.
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

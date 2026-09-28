# API Templates: an assistant that writes test data through the application's own endpoints

Design, agreed with the owner on 2026-09-28.

## 1. Why

Testers cannot write to the QA database, so every precondition - a draft
cycle, a cycle with three participants, a published cycle - is built by
hand through the UI before a case can run. That is slow, and it is the same
clicks every time.

The assistant already reads the database (`db_query`) and the developers'
code is on this machine. What it cannot do is create data. Direct SQL
writes would skip the application's validation, permissions and business
rules; the application's own endpoints do not.

So: the assistant reads a module's code, builds **operation templates** -
"Create a draft performance cycle" - as a named, parameterized sequence of
the calls the UI itself makes, proves each one works, and runs them with
new values before test execution. The app runs every call; the assistant
only picks a template and supplies values.

Success means the assistant can say "I'll run *Create a draft performance
cycle* with these values" before a test run and get back a new `cycleId`,
created through the real handlers - and the owner can see every template in
a tab and remove any they do not trust.

## 2. Owner decisions

1. The **assistant** is the consumer, through the MCP server, and it runs
   templates **before test execution**.
2. Operations may **create, edit and delete** data in the application
   under test.
3. A template the assistant writes may **run straight away** - there is no
   approval step. The user's control is removing templates.
4. A template is **proven before it is saved**: the assistant runs the
   draft end to end, and the app saves it only if every call succeeded.
   A changed template is proven again before it replaces the old one.
5. Templates live **on this machine only**. Sharing across the team may
   come later; the file format is self-contained so that it can.
6. Templates are **declarative JSON**, run by the app from inside a
   signed-in browser page (approach 1 of three considered - see §12).
7. The user can **view and remove** templates in a new tab, and nothing
   else: no edit, no run, no duplicate.
8. The feature is available **exactly where Auto Run is**: always in a
   development build, and in a release build only while this machine's
   optional extras are unlocked. The tab is invisible otherwise.
9. Database statements and API calls are logged to their **own folder**,
   separate from the app log.

## 3. What the spike found

Recorded against `hrmmainphdev01.phrsandbox.dev` while the owner created
and published cycle 272, and confirmed by reading the code.

- PMSV10 is a separate ASP.NET Core Razor Pages app (`net10.0`),
  `D:\PMS_Module\HRM-PMS-NET\src\PeoplesHR.PMS.WebUI`, hosted at
  `/hr/pmsv10`. The wizard is one page,
  `Pages\PerformanceCycle\Index.cshtml.cs` and its partials; each
  operation is a named handler: `POST /hr/pmsv10/performancecycle?handler=SaveProgress`.
  Bodies are JSON, except Cycle Setup, which posts a form. Saves return
  `{ success, cycleId, nextStepKey, redirectUrl }`.
- **Authentication is the legacy `/hr` session.** PMSV10 has no scheme of
  its own: System.Web Adapters remote-app authentication asks the legacy
  `/hr` app, on every request, who the caller is, from the forwarded
  `/hr` cookies (Forms auth plus the `ehrm85` cookie). The database
  connection itself is built from those claims.
- **The integration token does not help.** `IntAPIController.AuthClientUser`
  (`/api/v1/intapi/authclientuser`) issues a token in `HS_API_CLIENT_TOKEN`,
  read from an `auth_token` header, but only seven mobile-menu and
  localization actions accept it. Nothing in PMSV10 does.
- **Signing in without a browser is fragile.** The `/hr` login posts
  RSA-encrypted credentials (`jsencrypt`) with a JSON anti-forgery token,
  may stop at an OTP gate, and the session times out after 5 idle minutes.
- **Anti-forgery is on for every POST handler** (the Razor Pages default):
  a `RequestVerificationToken` header, taken from the page's hidden
  `__RequestVerificationToken` input, plus the antiforgery cookie.
- **Step order is not enforced server side.** Each handler works on its own
  given a `cycleId`; progress lives in `perf_cycle_step_progress`. Only
  `Publish` gates, needing Draft status and every required step complete.
- **Some GETs write.** Opening the evaluators step seeds evaluator roles;
  `TimelineState` regenerates stages. A template must make those GETs as
  the browser does.
- **Some saves un-complete later steps**: changing the performance method,
  the dates, or an evaluation-rule switch marks Timeline onward
  incomplete.
- IDs that differ per environment (rating methods, competencies, employee
  numbers) can be read from the database, which the assistant already
  can.
- Neither repo has Swagger, HTTP-level tests or seed scripts.

## 4. Template format

One JSON file per template, at
`<Auto Run data>/templates/<org-project slug>/<template-id>.json` - scoped
to the Azure DevOps project, as the sign-in recipes are
(`projects/<slug>.json`). A file is written only by a successful proving
run; it is never edited in place.

A run goes to the project's sign-in recipe `start_url` **origin**. Steps
hold paths only, never hosts; moving from dev01 to QA is a recipe change,
not a template change.

```jsonc
{
  "id": "pms-create-draft-cycle",
  "title": "Create a draft performance cycle",
  "module": "PMS / Performance Cycle",
  "effect": "create",                     // create | edit | delete
  "description": "Cycle setup + evaluation rules; leaves the cycle in Draft.",
  "sources": ["Pages/PerformanceCycle/Index.CycleSetup.cshtml.cs:95"],
  "antiforgery": { "page": "/hr/pmsv10/performancecycle?mode=create" },
  "params": [
    { "name": "cycleName", "type": "string", "required": true,
      "description": "Shown in the cycle list" },
    { "name": "startDate", "type": "date", "required": true },
    { "name": "ratingMethodId", "type": "number", "required": true,
      "lookup": "SELECT ... WHERE name = @ratingMethod" }
  ],
  "steps": [
    { "name": "Cycle setup", "method": "POST",
      "path": "/hr/pmsv10/performancecycle", "query": { "handler": "SaveProgress" },
      "form": { "CycleName": "{{cycleName}}", "StartDate": "{{startDate}}" },
      "expect": { "status": 200, "json": { "success": true } },
      "capture": { "cycleId": "$.cycleId" } },
    { "name": "Evaluation rules", "method": "POST",
      "path": "/hr/pmsv10/performancecycle", "query": { "handler": "SaveEvalRulesProgress" },
      "form": { "CycleId": "{{cycleId}}", "RatingMethodId": "{{ratingMethodId}}" },
      "expect": { "status": 200, "json": { "success": true } } }
  ],
  "outputs": ["cycleId"],
  "proven": { "at": "2026-09-28T10:14:00Z", "origin": "https://hrmmainphdev01.phrsandbox.dev",
              "account": "hr.admin", "outputs": { "cycleId": 273 } }
}
```

Rules:

- **Unknown fields are refused** (`deny_unknown_fields`), at every level.
- **`id`** is a filename component: ASCII letters, digits, `-`, `_`; at
  most 100 characters - `store::safe_run_id`'s character rule, shorter.
- **`effect`** is one of `create`, `edit`, `delete`. It is what the tab's
  badge shows; it is declared by the assistant, not inferred.
- **`params[].type`** is one of `string`, `number`, `boolean`, `date`
  (`YYYY-MM-DD`), `list` (a JSON array). `lookup` is guidance for the
  assistant only - **the app never runs it**.
- **Methods are `GET` and `POST` only.** That covers every PMSV10 handler,
  including `Manage?handler=Delete`, which is a POST.
- **`path`** must be relative and start with a single `/`. Absolute URLs,
  `//host`, a backslash anywhere in the authority position, and `..`
  segments are refused, using the same origin rules the recipe uses
  (`recipe::origin_of`). `query` values are URL-encoded by the app.
- **A step has at most one body**: `json` (any JSON value) or `form`
  (string values; sent as `multipart/form-data`, which is what Cycle Setup
  sends). File fields are not supported in v1.
- **Placeholders** are `{{name}}`, naming a declared parameter or a value
  captured by an **earlier** step. A placeholder that is the whole JSON
  value keeps the value's type (a number stays a number, a list stays a
  list); inside a longer string it is substituted as text. A placeholder
  naming nothing declared-or-earlier is refused when the template is
  checked, not when the step runs.
- **`capture`** maps a name to a path in the step's JSON response: `$.a.b`,
  `$.list[0]`, `$.list[*].id` (an array of every `id`). A capture that
  finds nothing fails the step.
- **`expect`** defaults to `{ "status": 200 }`. `json` is a partial match:
  every key given must be present with an equal value.
- **`outputs`** names captured values returned to the assistant.
- **`proven`** is written by the app from the proving run. A draft that
  carries one is refused.
- **`antiforgery.page`** is fetched once per run to read the token. Open
  point, settled by the first proving run: whether a plain GET of the
  wizard renders the token without the shell's `digest` parameter (§11).

## 5. The runner

One implementation, two modes: **prove** (a draft; saved only on success)
and **run** (a saved template).

1. **Checks, all reported together, before anything starts:**
   - the feature is available (dev build, or extras unlocked);
   - the **API templates (create/edit/delete)** switch on the AI Bridge tab
     is on - off by default, like database writes;
   - the template passes every rule in §4;
   - the account key exists in Auto Run's accounts, and the project has a
     sign-in recipe;
   - every required parameter is present and of its declared type, and no
     undeclared parameter is sent;
   - no other template run is in progress (one at a time, process-wide).
2. **Sign in** with the existing `autorun::signin::prepare` and `sign_in`,
   using the project's recipe and the chosen account: a saved session
   first, then the recipe. The browser runs headless
   (`launch::background_args`); the session is saved afterwards, as Auto
   Run does. A sign-in a headless browser cannot finish (an OTP prompt)
   stops the run with a sentence saying to sign that account in once from
   Auto Run.
3. **Token.** Navigate to `antiforgery.page` on the recipe's origin and read
   `input[name="__RequestVerificationToken"]`. No token: the run stops and
   names the page. The page turned out to be the login page (the session
   went stale): forget the session, sign in once more, try once more.
4. **Steps, in order.**
   - Rust builds each request - substitution, URL building and body
     encoding are pure functions, tested without a browser.
   - The page sends it with `fetch` (same origin, `credentials:
     "same-origin"`, the `RequestVerificationToken` header), evaluated
     through `browser::page::eval_value`. The token is passed into the
     page call and never returned from it.
   - The runner reads back the status, content type, final URL and body
     (capped at 64 KB).
   - It checks `expect`, then applies `capture`. **The first failure stops
     the run.**
   - A step whose final URL is the login page fails the run; the runner
     does not sign in again halfway through.
   - **No rollback.** A failed run reports exactly which steps landed and
     what they captured: `cycleId 274 created; failed at Evaluation rules:
     400 ...`. The application has no undo, and nothing is reversed
     automatically.
5. **Limits:** 30 seconds per step, 3 minutes per run.
6. **Results.**
   - **prove** writes the template file, with the app's `proven` block, only
     if every step passed. Replacing an existing id needs `replace: true`
     and a `why` (logged); without them, an existing id is refused. On
     failure nothing is written.
   - **run** returns the `outputs` and appends to the template's run
     history, `<template-id>.runs.json` beside it, capped at the last 20
     runs. The template file is not touched.
   - Both are logged (§6) and emit an event the tab listens for (§8).

## 6. Activity log

Database statements and API calls are logged to their own folder, separate
from the app log.

- **Where:** `<app log dir>/activity/`, one file per day per kind:
  `db-YYYY-MM-DD.jsonl` and `api-YYYY-MM-DD.jsonl`. One JSON record per
  line.
- **DB records:** the statement in full, the connection's name, the verdict
  (ran, or refused and why), rows affected or returned, duration.
- **API records:** template id, mode (prove/run), account key, origin, and
  per step: method, path, handler, status, duration, request and response
  bodies capped at 500 characters each. **Never** the anti-forgery token,
  a cookie, or an account's password.
- **`db_query`'s statement logging moves here.** Its `log_line` and
  `refusal_log_line` calls in `db/query.rs` stop writing SQL to the app
  log.
- **The app log keeps one summary line per call** - `db_query: write ran,
  3 rows, 120 ms`; `api template pms-create-draft-cycle: run ok, 9
  steps` - with no SQL and no bodies. A bug report still shows *that*
  something ran, not the data.
- **Bug reports do not include the activity folder.**
- **Retention: 30 days**, pruned at startup (the app log keeps 7).
- Settings -> Logs gains an **Open activity folder** button.

## 7. MCP tools

Offered only where the feature is available, gated in `mcp.rs` exactly as
the Auto Run tools are (the app must answer `true` for the feature; a
locked or older app offers none).

| Tool | Does |
|---|---|
| `get_api_template_guide` | The format contract (§4), placeholder and capture syntax, and the authoring workflow: read the module's code -> find values with `db_query` -> prove. Also this project's account keys and the recipe's origin, so the assistant never guesses a host. |
| `list_api_templates` | Every saved template for the current project: id, title, module, effect, params (types, descriptions), outputs, last run. |
| `prove_api_template` | `{ template, account, values, replace?, why? }` - run the draft; save it only if every step passed. |
| `run_api_template` | `{ id, account, values }` - return the outputs, or the failing step and what had been created. |

Deliberately absent:

- **No tool removes a template.** Removing is the user's, in the tab.
- **No ad-hoc request tool**, not even GET-only - some GETs write, and a
  free request would make the template rule meaningless. The assistant
  learns request shapes from the code and gets feedback from proving runs.

`prove` and `run` are refused while the switch is off, with a sentence
naming the AI Bridge tab; `guide` and `list` still answer. The AI Bridge
tab's tool list (`src/lib/mcpTools.ts`) gains the four tools.

## 8. The tab

- **API Templates**, in the sidebar with Auto Run's "In Dev" pill,
  **placed after AI Bridge** so every existing tab keeps its Ctrl+number.
  It has no number of its own (Ctrl 1-9 are taken while Auto Run shows).
- **Invisible exactly as Auto Run is**: absent from the rail, the command
  palette and the shortcut list unless the build is a development build or
  extras are unlocked; if extras are reset while it is open, the App
  shell's existing Auto Run redirect moves the user off it.
- A new glyph in `src/lib/actionIcons.ts`, named for what the screen does.
- **Header:** the Azure DevOps project; the environment the templates run
  against (the recipe's origin host); a pill showing whether the API
  templates switch is on, linking to the AI Bridge tab.
- **List**, grouped by `module`. Each row: title, an **effect badge**
  (create / edit / delete in the `success` / `warning` / `danger` tokens),
  parameter count, "proven 28 Sep as hr.admin", and a dot and time for the
  last run. A search box filters by title, module or id.
- **Expanded row, read-only:** description; parameters table (name, type,
  required, description, lookup hint); the steps in order (method, path +
  handler, body field names, expect, captures); `sources` as `file:line`;
  the proving evidence (outputs it created); the last 20 runs (time,
  account, ok or failed-at-step, outputs).
- **One action - Remove.** A confirmation dialog names the template and its
  effect. Removing deletes the template file and its run history from this
  machine; there is no undo, and the assistant can re-prove it.
- **Live:** the list refreshes when a prove or run finishes, from an event
  Rust emits (as the AI Bridge badge does).
- **Empty state:** a sentence saying the assistant builds these from the
  code, and a pointer to the AI Bridge tab to connect one and turn the
  switch on.
- Colours from tokens only; `src/ui-consistency.test.ts` passes unchanged.

## 9. Errors

Every failure goes back to the assistant and, for a run, into the
template's run history.

- **Before any request** (all problems in one answer): locked; switch off;
  a malformed template (unknown field, absolute path, other method, a
  placeholder naming nothing, a bad capture path, a `proven` block sent in);
  missing or mistyped parameters; unknown account or recipe; another run in
  progress.
- **During a run:** the browser could not start; sign-in needs a person;
  the token page has no token; a step failed (status, `expect` mismatch,
  empty capture); a step or the run timed out.
- **Each failure message** names the step and its handler, includes a
  response excerpt of at most 500 characters, and lists what had already
  been created. It never includes a raw browser or transport error - those
  go to the app log, the same rule as `ado/transport.rs`.

## 10. Testing

Rust - a new `src-tauri/tests/suite/api_templates.rs`, with its `mod` line
in `suite/main.rs`:

- Template checks, one test per rule in §4.
- Substitution: whole-value typed, inside-string text, captured values,
  order (a later capture cannot be used earlier).
- Capture paths: `$.a.b`, `$.list[0]`, `$.list[*].id`, missing -> failure.
- `expect` partial matching.
- Origin enforcement: every refused path shape.
- The runner against a fake `Driver`: the full success path; failure at
  step N reporting what was created; a stale session at the token page
  signing in again once; a login redirect mid-run failing.
- Storage: written only on success; `replace` + `why` required for an
  existing id; run history capped at 20; remove deletes both files.
- Activity log: DB and API detail lands in `activity/`; the app log's
  summary line carries no SQL and no body; nothing token- or
  cookie-shaped is ever written. These take the lock from
  `suite/serial.rs`.
- MCP: the four tools absent while locked; `prove`/`run` refused while the
  switch is off.
- `tests/bindings.rs`: no anti-forgery token or cookie field appears in
  the generated TypeScript.

Frontend (vitest):

- The tab hidden when locked, shown in dev; the redirect on reset; the
  existing Ctrl numbers unchanged.
- Grouping, search, expanding a row, the Remove confirmation, live refresh
  on the event, the empty state.
- `ui-consistency.test.ts` unchanged; `docs-site/src/guard.test.ts`'s term
  list gains the tab name and the four tool names, so How To Use never
  mentions them.

Acceptance: the assistant proves one real template - **Create a draft
performance cycle** - against dev01, then runs it once with new values.

## 11. Open point

- **The anti-forgery page.** The wizard is normally opened through the
  `/hr` shell with a `digest` parameter
  (`/hr/PMSV10/PerformanceCycle?mode=Create&mvc=1&digest=...`). The first
  proving run settles whether a plain GET renders
  `__RequestVerificationToken`. If it does not, `antiforgery` gains a way
  to get the digest (`/hr/CommonComponents/Common/GetUrlDigest`, seen in
  the recording) as its own first request.

## 12. Approaches considered

1. **Declarative JSON, sent from a signed-in browser page** - chosen.
   Sign-in needs a browser anyway, Auto Run already has the driver, recipe
   and sessions, and a page `fetch` carries exactly the cookies, headers
   and origin the UI sends.
2. **The same format, sent from Rust with `reqwest`** using cookies lifted
   from the saved session. Faster, but with 5-minute sessions the browser
   fallback would be the normal path, and it duplicates cookie and
   anti-forgery handling the browser does correctly. The format does not
   change if the transport moves here later.
3. **Assistant-written scripts in a sandbox.** Most flexible; turns the app
   into a code runner that is hard to show, hard to confine to "these
   calls, these hosts", and hard to prove safe.

## 13. Not in v1

- Sharing templates across machines or testers.
- File uploads in form bodies.
- Methods other than GET and POST.
- A Run button for the user.
- Running a template from Auto Run or the runner before a case.

## 14. Noticed in passing (not this app)

`TenantResolutionMiddleware.cs:16-24` in PMSV10 trusts a client-supplied
`X-Forwarded-Host` to choose where the remote-app authentication call goes,
before `UseForwardedHeaders` and with no allow-list. A caller could send
that call, and the `RemoteAppApiKey` it carries, to a host they control
and return any user and database claims. Raised with the owner to pass to
the PMS team. This design does not use it.

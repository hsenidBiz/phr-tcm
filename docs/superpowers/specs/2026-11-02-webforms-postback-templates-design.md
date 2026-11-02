# API templates: WebForms postback steps - design

Date: 2026-11-02. Status: draft for the owner's approval.

## Intent

PeoplesHR's newer screens save through small JSON endpoints, which API
templates already replay. Older screens (`CompanyEvaluation.aspx`,
`RatingMethod.aspx`, `SectionItem.aspx` among them) are ASP.NET WebForms:
saving posts the whole form back to the page, with the page's state
(`__VIEWSTATE`, `__VIEWSTATEGENERATOR`, `__EVENTVALIDATION`) from a fresh load
of that page in the same session, the clicked button in `__EVENTTARGET`, and
it answers with the whole HTML page. Work often takes several round trips
(Save, then Create to make item rows, then fill and Save Item), each needing
the state the previous answer carried. Templates cannot do this today, so
setting data up on those screens means clicking through them.

The owner wants templates to do WebForms postbacks too, for both uses:
reliable replay to set data up, and a properly checked answer when the save
itself is what is tested.

## Goals

- A template step can be a postback to a WebForms page: TCM carries the
  page state itself; the template names only the page, the button and the
  fields that matter.
- Several postback steps in one template chain their state: each posts
  with the state from the answer before it.
- A step can check and capture from an HTML answer (not only JSON).
- Everything templates already have keeps working with postback steps:
  params and placeholders, proving runs, fixtures, flows, outputs, run
  history, the guide, the assistant tools.

## Non-goals

- Recording a postback while the person clicks and turning it into a
  template (approach 3 below). A follow-up once this exists.
- Driving the real form in a browser (approach 2): that is Auto Run.
- ASP.NET AJAX partial postbacks (`ScriptManager` / `UpdatePanel` delta
  answers, `|`-delimited). Only full postbacks. If a target page only
  saves through an UpdatePanel, that is reported, not worked around.
- Any change to how sign-in, sessions or the held browser work.

## Anti-goals

- A postback must never send stale or guessed state. If the page's state
  cannot be read, the step fails before sending anything.
- No secret or page state in any report, log, activity record, answer or
  UI: `__VIEWSTATE`, `__EVENTVALIDATION` and the anti-forgery token are
  treated like the token is today (never formatted into a string; shown as
  a fixed marker where a step's sent body is shown).
- No URL in user-facing errors (the existing rule).

## Constraints

- Steps run as a `fetch` made by a signed-in page (runner.rs), so cookies
  are attached by the browser; postbacks keep that.
- The template format is `deny_unknown_fields` and saved on disk; a template
  saved before this must load and write back unchanged.
- `src/bindings.ts` is generated; the TS editor for templates shows steps.
- Release gates: role caps, docs guard, ui-consistency.

## Approaches considered

1. **A postback step in templates** (chosen). The runner loads the page (or
   reuses the previous answer), copies its hidden state fields, adds the
   template's fields and the button, posts, and keeps the answer for the
   next step. Captures and checks gain HTML forms. Fits fixtures, flows,
   proving and placeholders as they are.
2. **Drive the real form in a hidden browser.** State takes care of itself,
   but it is Auto Run by another name: slower, tied to layout, duplicates
   components.
3. **Record and replay.** Least writing per template, but needs 1
   underneath. Later.

The owner picked 1, with 3 as a possible follow-up.

## The design

### Step shape

A step gains an optional `postback` object. A step with `postback` is a
postback step; `method`, `json` and `query` do not apply to it (a postback
is always a POST of `application/x-www-form-urlencoded`, or multipart when
it has `files`).

```json
{
  "name": "save the evaluation",
  "postback": {
    "page": "/Perf/CompanyEvaluation.aspx",
    "button": "CmdSave",
    "argument": ""
  },
  "form": {
    "txtEvalName": "{{name}}",
    "dtStartDate": "{{start}}",
    "chkGoal$0": "on"
  },
  "expect": { "status": 200, "html": { "shows": ["Saved"], "hides": ["#ValidationSummary1"] } },
  "capture": { "evalId": "html:#hdnEvalId@value" }
}
```

- `page`: the page's path, a safe relative path like every step path.
  Query strings allowed (`?id={{evalId}}`), placeholders allowed.
- `button`: the control that is "clicked". Sent as `__EVENTTARGET` with
  `__EVENTARGUMENT` = `argument` (default empty) for a link-style or
  `__doPostBack` control. For a real submit button (`<input type=submit
  name=...>`), the runner instead sends `name=value` of that button and an
  empty `__EVENTTARGET` - it decides which by reading the page (the control
  with that name, or whose name ends in `$<button>`, is a submit input or
  not). If the page has no such control, the step fails before sending.
- `form`: the template's fields, as today. Names are the controls' `name`
  attributes (with `$`). Placeholders as today.
- `files`: as today (multipart).

### Where the state comes from

Each run keeps "the current page" per page path: the HTML of the last
answer for that page in this run.

- First postback to a page in a run: the runner GETs the page (a `fetch`
  from the signed-in page, same as a step) and uses its HTML.
- A later postback to the same page (same path, ignoring query) uses the
  previous answer's HTML - this is what carries Create's rows into Save
  Item.
- A postback to a different page starts from a fresh GET of that page.
- A step may say `"fresh": true` in `postback` to force a fresh GET.

From that HTML the runner reads every `<input type="hidden">` whose name
starts with `__` (state and validation fields; `__EVENTTARGET` and
`__EVENTARGUMENT` are then set by the step), plus the current value of every
other control in the page's form (inputs, checked checkboxes/radios,
selected options, textareas) - so a postback sends what the browser would
send. The template's `form` fields then replace or add to those values.
Sending the page's own values for untouched controls is what WebForms
expects; without them it can treat a missing field as cleared.

Parsing happens in the page (DOMParser inside the browser), not in Rust, so
the state never leaves the page as a string. Only the field NAMES (not the
values of `__` fields) come back to Rust for checks and reports.

If the page's HTML has no `__VIEWSTATE` field, the step fails before
sending: "<page> is not a WebForms page - use a normal step."

### The answer

- A postback answer is HTML. A 302 to the sign-in page, or an answer that
  is the sign-in page, is the existing "signed out" error.
- An ASP.NET error page (yellow screen, `<title>` "Runtime Error" or a
  "Server Error in" heading) is a failed step with the error's title only.
- `__EVENTVALIDATION` failures ("Invalid postback or callback argument")
  are a failed step saying a field or button value is not one the page
  offered, naming the field if the message names it.

### HTML checks

`expect` gains `html` (only on a step whose answer is HTML):

- `shows`: texts that must appear in the page's visible text, or CSS
  selectors that must match something (a string starting with `#`, `.` or
  `[`, or containing `>` is a selector; anything else is text).
- `hides`: the same, that must NOT appear / match. For "no validation
  message", e.g. `.validation-summary-errors`, or the page's summary id.
- `rows`: `{ "table": "<selector>", "has": "<text>" }` - a row of that
  table holds the text (for "the grid has the new row").

Status stays as today (default 200).

### HTML captures

A `capture` value starting `html:` reads from the HTML answer:

- `html:<selector>` - the element's trimmed text.
- `html:<selector>@<attribute>` - the attribute (`@value` for an input).
- `html:<selector>@value` on a `select` - the selected option's value.

Exactly one element must match, or the capture fails naming the selector
and the count. `$...` captures keep meaning JSON.

A capture of a `__` state field is refused at save: state is carried by
the runner, never by a template.

### Reports, history and secrets

The step report's sent body lists every field NAME it sent, with the
template's own fields' values (scrubbed as today) and `__` fields shown as
`(page state)`. Answers are excerpted as today, with hidden `__` inputs'
values replaced by `(page state)` before any excerpt is made. The
anti-forgery token rule is unchanged.

### Anti-forgery and the token page

A template's `antiforgery.page` stays required. For a WebForms-only
template it is set to the first postback page; the runner reads a token
if one is there and does not need one when the template has only postback
steps (WebForms pages carry their own validation).

### The assistant side

- `prove_api_template` / `run_api_template` / `save_api_template` (and the
  fixture and flow tools) take postback steps unchanged in their
  signatures.
- The API templates guide (api_templates/guide.rs) gains a section: how
  to tell a WebForms page (form posts to itself, `__VIEWSTATE`), how to
  read control names off the live page with the Auto Run page tools, one
  postback per round trip, `html` checks for the answer, a worked
  three-step example (Save, Create, Save Item).
- `send_api_request` (one-off) gains the same `postback` option, so an
  assistant can try a postback before saving a template.

### The app's UI

The API Templates screen shows a postback step as "Postback to <page> -
<button>" with its fields; its sent-body view shows `(page state)` for the
`__` fields. No new screens.

## Left to the builder

- Exact Rust types and where the in-page JS for reading/posting lives
  (beside `FETCH_FN` is natural).
- How selector-vs-text is decided in `shows`/`hides` if the rule above
  proves ambiguous in tests (an explicit `{ "selector": ... }` form is an
  acceptable alternative).
- Test fixtures: a small fake WebForms page served by the existing fake
  site helpers, with view state that changes per answer and a Create step
  that adds rows.

## Settled after the builder check (my calls)

1. **Where the carried page lives.** In the page, not in Rust. The page
   that runs the steps keeps a per-run map of page path -> the last answer's
   full HTML (a JS variable beside `FETCH_FN`). Reading the state, building
   the post, the `html` checks, the `html:` captures and the `(page state)`
   scrubbing all run there with DOMParser. Only field names, capture values,
   check results and an already-scrubbed excerpt come back to Rust. No HTML
   parser crate is added. The runner does not navigate the tab between the
   steps of a run; if the carried page is ever missing where an earlier step
   of the same run left one, the step fails ("the page this step carries on
   from was lost - run the template again") rather than loading a fresh one,
   because a fresh load would drop rows an earlier step made.
2. **Redirects.** For a postback step only: a redirect to the sign-in page
   is the existing "signed out" error; a redirect to any other page of the
   same site is followed (as `fetch` does) and is a pass if the final answer
   meets `expect`. The final page's HTML becomes the carried page for the
   FINAL path (query ignored), so a Save that lands on `?id=N` carries into
   the next step on that page. Other step kinds keep today's rule.
3. **Size.** The whole answer is kept in the page for state, checks and
   captures. The 64 KB cap applies only to what is reported. A page over
   8 MB fails the step ("the page is too large to carry").
4. **One-off postbacks** (`send_api_request`) carry from the previous one-off
   postback in the same held browser, while that browser is held and the
   page the requests run in has not navigated; `fresh: true` forces a load.
   The answer says whether it carried on or loaded the page fresh. So an
   assistant can try Save -> Create -> Save Item one call at a time.
5. **Reach and retries.** Carried state belongs to one template run: it
   starts empty for each template, including each template of a fixture or
   flow (they pass ids through outputs, as today). A postback step is never
   retried automatically: one that timed out may already have saved, so it
   fails and says so.

## Calls I made (owner to check)

- Untouched controls send the page's own values (as a browser would).
- Same page path = carry the previous answer; a different page = fresh
  load; `fresh: true` forces a load.
- Full postbacks only; UpdatePanel partial postbacks are out of scope.
- Captures of page-state fields are refused.

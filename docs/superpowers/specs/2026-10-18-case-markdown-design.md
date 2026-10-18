# Formatting in test case text

Date: 2026-10-18. Status: design, from Avin's request and answers (2026-10-18).

## Intent

Test cases written by an assistant are hard to read when everything is one paragraph: a
prerequisites list run together with " • " separators, an SQL query squeezed onto the end of a
sentence. Azure DevOps can show formatting in a test case, and TCM should let the assistant (and
the person) use it - **meaningfully, not excessively** - so a case reads easily in Azure DevOps
and in TCM alike. Code and SQL get a code block where one can be shown, and their own lines where
it cannot.

## What Avin said and showed (2026-10-18)

1. Azure DevOps supports Markdown in test case text; TCM does not, today.
2. The **List of Prerequisites** field is a rich-text field: its toolbar has bold, italic,
   underline, bullet and numbered lists, code and more, and it offers "We support markdown, you
   can convert this field" (a per-work-item, one-way switch to Markdown storage).
3. **Steps** (action and expected result) offer only **bold, italic and underline**.
4. A query in a step goes **on its own lines, plain but readable**, as the Calculation Engine
   cases did ("Run the following query ..." then the query).
5. The formatting should show in the app where test cases are viewed and edited.

## What exists today

- Steps are written as HTML-escaped plain text in `<parameterizedString isformatted="true">`
  (`src-tauri/src/steps_xml.rs`), so typed Markdown shows literally in Azure DevOps. Reading
  strips the tags and **collapses all whitespace**, so line breaks and bullets are lost (Avin's
  screenshot: step 1's prerequisites read back as one line).
- Prerequisites (a project field matched by name - `preconditionsRef`, `src/lib/fieldPrefs.ts`)
  are written as `<div>` + escaped text (`ado/endpoints.rs`), read back by `html_to_text`, which
  keeps `<li>` as "• " and block ends as newlines.
- Saving changes only the steps that changed (`steps_patch` / `merge_steps_xml`), so a step left
  alone keeps whatever formatting Azure DevOps holds; an edited step loses it.
- The app shows case text as plain text (`CaseDetail`, `CaseStepsTable`, the step and
  precondition editors). Markdown is already rendered safely elsewhere: `src/lib/markdown.ts`
  (`marked` + DOMPurify) in the webview, `src-tauri/src/markdown.rs` (pulldown-cmark, raw HTML
  dropped) for TCM's own pages.
- The writing guide (`get_writing_guide`, in `ai_bridge.rs`) says nothing about formatting;
  `validate_cases` warns about literal HTML tag names in text.

## The design

**One idea: TCM's text is Markdown; Azure DevOps keeps HTML.** Inside TCM - the import JSON, the
queue, the editors, the assistant's tools - a step's action and expected result and the
prerequisites are Markdown. TCM turns it into the HTML each Azure DevOps field supports when it
saves, and turns that HTML back into Markdown when it reads. No field in Azure DevOps is converted
to Markdown storage (that switch is one-way and per item; HTML already shows everything needed -
my call).

### What each field may hold

| Markdown | Prerequisites (HTML out) | A step's action / expected (HTML out) |
|---|---|---|
| `**bold**` | `<strong>` | `<B>` |
| `*italic*` | `<em>` | `<I>` |
| line break / paragraph | `<p>`, `<br>` | `<BR/>` |
| `- item` / `1. item` | `<ul>`/`<ol>` | one line per item, "• item" / "1. item" |
| `` `code` `` | `<code>` | the text, plain |
| ```` ```sql ```` fence | `<pre><code>` | its lines, plain, each on its own line |
| headings, tables, links, images, quotes, raw HTML | as plain text | as plain text |

Underline has no Markdown form: Azure DevOps underline read back becomes plain text (my call).

### Reading back

HTML from Azure DevOps becomes Markdown by the same table in reverse (`<B>`/`<strong>` → `**`,
lists → `- `, `<pre>` → a fence, `<BR/>`/`<P>`/`<DIV>` → line breaks). Text that holds no
formatting reads back exactly as written - a `COUNT(*)` must not turn into italics or into
`COUNT(\*)` - so plain cases (every case written so far) look and save as before.

Steps keep their line breaks when read (today they are collapsed). Everything that compares step
text - the duplicate-title warning, the seen check, Auto Run's step texts, the Playwright export's
table cells (newlines already become `<br>`) - must keep working; where a comparison needs one
line it collapses whitespace itself.

### The assistant

The writing guide gains a short "Formatting" section: use formatting only where it helps a reader -
bold for the one value or control a step turns on, a list for several prerequisites, a fenced code
block for any SQL or code (in a step its lines are kept plain, in the prerequisites it shows as
code); never headings, tables or emphasis on whole sentences; never more than a few bold words in
a step. `validate_cases` warns (does not refuse) on: a heading, table, link or image in a step or
prerequisites; more than 3 bold or italic spans in one step; a step whose text is mostly bold; an
SQL-looking line (`SELECT`/`EXEC`/`UPDATE`/`INSERT`/`DELETE` ... `FROM`/`WHERE`/`SET`) outside a
code fence. The existing literal-HTML warning stays.

### The app

- Viewing: prerequisites and steps render their Markdown (the existing `renderMarkdown` and its
  sanitiser) in the case views, the queue, the existing-case editor's read view and TCM's review
  page (`import_parser/html.rs`, through `markdown::to_html`).
- Editing: step fields and prerequisites become multi-line text areas that keep the Markdown as
  typed, with a Preview toggle; no toolbar (my call - YAGNI).

### Saving only what changed (my calls, after the builder check)

- **Prerequisites follow the steps' rule.** An update leaves the prerequisites field out unless
  its text changed: the save compares the new Markdown with the field's current value read as
  Markdown (fetched for the update, as the steps' original XML already is). Editing only a title
  never rewrites the prerequisites.
- **Old one-line text is "unchanged".** Text saved before this change (queue drafts, caches,
  exported JSON, `.test-cases` files) holds steps read with their whitespace collapsed. A step (or
  the prerequisites) counts as unchanged when the new text equals the current value **or equals
  the current value with its whitespace collapsed**; so stale one-line copies never rewrite a
  step that Azure DevOps now holds with line breaks or formatting, and a real edit (one that adds
  line breaks or formatting) still differs from both and is saved.
- **No guessing on the way back.** A step's plain query lines read back as plain lines (not
  re-fenced), lists as "• " lines. `validate_cases`' formatting warnings look only at text that is
  new or changed, so re-reading and saving an old case raises none.
- **A field someone converted to Markdown storage.** When a work item reports the prerequisites
  field as Markdown (`multilineFieldsFormat`), TCM writes its Markdown as-is and reads it as-is;
  otherwise HTML as above.

### Everywhere case text goes (my calls)

- Shown read-only anywhere in the app (case views, queue, Run Tests' case preview, the Runner
  window, Auto Run's case text): rendered Markdown through the existing sanitiser.
- Sent to another HTML field (a Bug's repro steps from a failed run, `commands/bugs.rs`): turned
  into HTML by the prerequisites table, so no Markdown symbol shows in the Bug.
- Exported to the Playwright repo: left as Markdown (the files are Markdown).

## Non-goals

- Converting any Azure DevOps field to Markdown storage.
- Underline, colours, fonts, tables, images or attachments in case text.
- Changing Shared Steps.

## Anti-goals

- A case that renders as plain text today renders differently, or saves differently, after this
  change without anyone editing it.
- Markdown symbols showing literally in Azure DevOps (`**`, `- `, ```` ``` ````).
- Excessive formatting from the assistant: bold everywhere, headings in steps.
- Any HTML from Azure DevOps or an import rendered unsanitised.

## Constraints

- Steps XML only through `steps_xml.rs` (step ids start at 2); merge-only saves unchanged.
- Rust tests under `src-tauri/tests/suite/`; the UI consistency gate never weakened; no new
  dependency where `pulldown-cmark`, `marked` and DOMPurify already serve.
- The How To Use guide gets a sentence where test case editing is described.

## Left to the builder

- The exact HTML Azure DevOps' step editor emits (`<B>` vs `<STRONG>`, `<DIV>` vs `<P>`), read
  either way.
- How plain text is protected from being read as Markdown on the way back (escape only what would
  otherwise be read as formatting).
- The editor's look within the app's conventions.

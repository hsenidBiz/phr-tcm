# Spec pane in the browser review page - design

The review page a developer opens with View in Browser shows the test
cases and, beside them, the specification documents those cases were
written from - markdown files on disk or Azure DevOps wiki pages - in a
resizable right-hand pane with one tab per document, VS Code style. A
reviewer note's `Spec: <file> <section>` citation becomes a link that opens
the right tab at the right heading, so the quote and its source sit in one
window.

## Where the documents come from: the `specs` field

- A top-level `"specs"` list in the JSON file, next to `format`, `version`
  and `instructions`. Each entry is a string: a file path (absolute, or
  relative to the JSON file's directory) or an Azure DevOps wiki URL
  (`https://dev.azure.com/{org}/{project}/_wiki/wikis/{wiki}/{id}/{slug}`
  or the `?pagePath=` form - what the browser's address bar shows).
- Written by the assistant: the intake already makes it name the
  authoritative documents (`spec_paths`), and the writing guide gains a
  `## specs` section telling it to copy those into `specs` - wiki URLs
  as-is. The export `instructions` string names the field.
- Read by the importer into the parsed file (`ImportResult.specs`), kept by
  the app per watched file, and preserved by every write-back the app does
  (bulk edits and comment saves already keep unknown top-level keys; this
  one is known and kept the same way). `transform_cases` never touches it.
- Editable in the app: on the Import File tab each watched file lists its
  specs with **Attach spec…** (file picker, `.md`/`.txt`, multiple), **Add
  wiki link** (a pasted URL) and a × per entry. The list is written into the
  JSON's `specs` through the same read-patch-write path the general comment
  uses, so it travels with the file. A picked file under the JSON's own
  directory is stored relative; anything else absolute.
- Only the draft page (a queue assembled from files) carries specs; a
  received share keeps none, since its paths name the sender's disk. View
  Test Cases (cases from Azure DevOps) has none either and shows no pane.

## Resolving and rendering (Rust, when the page is written)

`spec_pane.rs`:

- `SpecSource::from(entry, base_dir)`: an `http(s)://` string containing
  `/_wiki/wikis/` is `Wiki(url)`; anything else is `File(path)` joined to
  `base_dir` when relative.
- `SpecDoc { title, kind: "file" | "wiki", source, html, error }`:
  - File `.md`: `markdown::to_html` of the file; title is the first `# `
    heading if any, else the file name. `.txt`: `<pre>` of the escaped
    text; title the file name. Other extensions: `error = "Not a text
    spec"`, no html. Unreadable: `error = "Could not read <name>: <reason>"`.
  - Wiki: org and project come from the URL itself (`/{org}/{project}/_wiki`),
    the page through the existing `AdoClient::get_wiki_page` with the URL
    as `path` and the signed-in user's token. Title is the page's path's
    last segment (`-` read as space). Before rendering: `[[_TOC_]]` lines
    dropped; an image whose target is not `http(s)://` is rewritten to a
    link `[alt (image)](<page URL>)` - attachments need the user's session,
    which a file on disk cannot carry. Fetch failures give `error = "Could
    not fetch this wiki page: <the client's user-facing sentence>"`.
    Successful fetches are cached in the Rust cache under
    `wiki-page:<url>` for 10 minutes, so the keep-in-step refresh does not
    re-download on every focus.
  - Duplicates (the same resolved source from two files) render once.
- `render_specs(files: &[DraftFile], token) -> Vec<SpecDoc>` is what the
  draft page commands call; the queue page (Azure DevOps cases) passes none.

## The pane (page)

- Markup, only when at least one `SpecDoc` exists: the `.shell` grid gains
  a `side` column holding the existing General comments box (when there
  are files) above `<section class='specs' id='tc-specs'>` with a tab strip
  (`<button class='spec-tab' data-spec='i'>` per doc, a small `wiki` mark
  on wiki tabs, an "Open in Azure DevOps" link in the wiki doc's header),
  one `<article class='spec-doc' data-spec='i'>` per doc (hidden unless
  active), and a `.spec-grip` on the pane's left edge.
- Width: `--spec-w` (default 520px, min 320px, max 60vw) dragged by the
  grip, kept in `localStorage` (`tcm-report-spec-w`), double-click resets.
  Below 1120px the pane stacks under the cases.
- The sticky search bar gets a `#tc-spec` chip, "Hide spec" / "Show spec",
  remembered like the notes toggle (`tcm-report-spec-off`).
- The active tab is remembered per page kind and file set
  (`tcm-report-spec-tab:<sources joined>`); the pane scrolls independently
  of the cases (sticky, `max-height: calc(100vh - 24px)`).
- Live update: the pane is inside `.shell`, so the swap replaces it; the
  script saves the active tab, each doc's scroll position and the toggle
  before the swap and restores them after, the same way it restores the
  search text and open cases.
- Heading anchors: on wire-up, every `h1`–`h4` in a doc gets an id made
  from its text (`spec-<i>-<slug>`); duplicates get `-2`, `-3`.

## Citations become links

`cases-specs.js` (new, embedded after `cases-page.js`; its pure helpers
sit on `window.tcmSpecs` for tests):

- `splitCitation(text)`: from a `Spec: …` line, the document (up to and
  including the first token with a file extension, or its first word when
  none) and the section (the rest, with a trailing `> "…"` quote and a
  `- no quotable text (…)` exemption removed).
- `findSpecTab(docs, document)`: the doc whose title or source file name
  equals the document ignoring case and extension; else contains it; else
  none.
- `matchHeading(headings, section)`: the heading whose leading number
  matches the section's leading number (`5.8` matches `5.8 Display Rules`
  and `5.8.` but not `5.80`); else the heading sharing the most words with
  the section (case-insensitive, at least two, or one when the section is
  one word); else none.
- Wire-up: every text node under `.rev` (reviewer notes) whose line starts
  with `Spec:` is wrapped in `<a class='spec-link' href='#'>`; click opens
  the tab (showing the pane if hidden), scrolls the heading into view and
  flashes it (`.spec-flash`, 1.2 s). No matching tab → the link is inert
  and titled "No spec tab for <document>".

## App side

- `WatchedFile.specs?: string[]` loaded with the general comment at import
  and on every re-parse; `DraftFile.specs` carries it to Rust; the
  QueueSection view/refresh mutations pass it.
- Commands: `read_specs(path) -> Vec<String>`, `save_specs(app, path,
  specs) -> String` (new fingerprint), under the same `NOTE_WRITE` lock as
  the comment saves. `view_draft_html` and `refresh_draft_html` become
  async (the wiki fetch needs the token) with unchanged parameters.
- The Import File tab's file row gains the specs list and controls (above).

## Testing

- Rust `tests/specs_field.rs`: `read_specs`/`patch_specs` on the round-trip
  document (order kept, non-strings ignored with a warning, bare-list file
  refused with the comment's message), `parse_file` returns `specs`, a
  patched file keeps every other key.
- Rust `tests/spec_pane.rs`: file `.md` renders with the first heading as
  title, `.txt` as `<pre>`, missing file gives its error doc, relative path
  resolves against the JSON's directory, wiki URL yields org/project/wiki/id,
  a wiremock wiki page renders with `[[_TOC_]]` dropped and an attachment
  image rewritten to a link, a 404 gives the fetch error doc, duplicates
  collapse.
- Rust `tests/import_parser.rs`: the page has `#tc-specs` with one tab per
  doc only when docs are given; an error doc shows its message; the spec
  chip is in the sticky bar; no docs → no pane and no chip.
- Vitest `src/lib/casesSpecs.test.ts` on `window.tcmSpecs`: `splitCitation`,
  `findSpecTab`, `matchHeading` per the rules above.
- Vitest: Import File attach flow writes `specs` through `saveSpecs` and
  patches the watch; QueueSection passes `specs` in `files`.
- Guide test: `## specs` present.
- Manual walk: the Calculation Engine JSON with its spec file and a wiki
  link; drag the grip; hide/show; click a citation; refresh keeps the tab.

## Deferred

- Word / PDF specs.
- Wiki search from the Attach control (the `search_wiki` tool exists).
- Editing the spec in place.

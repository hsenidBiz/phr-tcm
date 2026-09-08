# Round 8 feedback — design

**Date:** 2026-09-08
**Status:** approved in conversation
**Source:** `D:\Test Case Writing\tcm-testcases-mcp-feedback-round8.md` (five items, §1–§13)
**Scope:** the `tcm-testcases` MCP tools, one app-side hazard in the watched
folder, and the generated `tcm:fanout` skill — all in this repository.

## Two places the feedback is wrong about the code

Both are stated here so nobody implements a fix for a defect that does not
exist.

**§10 "`replace_in_*` replaces only the first occurrence per field" — false.**
`transform.rs:662–672` uses Rust's `str::replace`, which replaces every
occurrence. The existing test (`tests/transform.rs:290`) plants one
occurrence, so it cannot prove it either way. What almost certainly left
cases behind is that `replace` is **case-sensitive**: "Appraisee" →
"Employee" skips "appraisee". The fix is a test that pins replace-all, plus
a report line with the occurrence count and a hint when the find string has
case variants in the draft — not a new replace-all.

**§11 is not a snapshot feature — it is the copy-in.** `workspace.rs:58`
`copy_into_cases` copies a file picked from outside `.test-cases` into it
and **never overwrites**: identical bytes reuse the existing file, different
bytes take `name-2.json`, `name-3.json`. So the canonical name keeps the
*first* bytes it ever received and every later pick of the corrected master
lands at a higher suffix — exactly the table in §11.
`tests/workspace.rs:57` pins this as intended. It was a data-safety choice
with the naming backwards.

## 1. Citations (§1–§7, §9)

### Guide
`ai_bridge.rs:1388` (`get_writing_guide`, the reviewer_notes paragraph):
state the order once — *the `Spec:` pointer comes first, with the quote
directly beneath it as `> "..."`*. The current sentence ("Alongside the
`Spec:` pointer, quote the source sentence verbatim") reads as
order-agnostic and is what all five slice writers worked from.

### Advisories
Both places that report a bare citation share the parser
`speccov::parse_citations`:
- `ai_bridge.rs:797` — `validate_cases` advisory *"cites a Spec: section
  with no verbatim quote and no exemption"*.
- `speccov.rs:765` — `check_spec_coverage`'s `cited_without_quote`.

When the case has a bare citation **and** the notes contain a `>`
blockquote line anywhere, the message becomes positional: *"a quote is
present but not where the checker reads it — put the `Spec:` line first
and the quote directly beneath it as `> "..."`; a table or code block is
not a quote: use `Spec: <file> <section> - no quotable text (table/diagram
| code-not-prose)`"*. With no blockquote at all, the current message
stands. Detection is a helper next to `parse_citations`
(`has_blockquote(notes) -> bool`), not a second regex in each caller.

### `normalise_citations` — new `transform_cases` op
Takes only an optional `where`. Per matched case, on `reviewer_notes`:

1. Locate `Spec:` lines (`speccov`'s `spec_re`) and blockquote runs
   (consecutive lines starting `>`; a run may be one line).
2. **Exactly one of each** → rebuild as: prose before either / the `Spec:`
   line / the quote as `> "<text>"` (quotation marks added if missing, a
   multi-line run joined with single spaces) / every other line, in its
   original order. Blank-line separation as in the accepted example.
3. **Unquotable block** — first line of the run starts with `|`, or the
   run reads as code (starts with `--`, contains a fence, or begins with a
   SQL keyword `SELECT|INSERT|UPDATE|DELETE|CREATE|ALTER|EXEC|DECLARE|WITH`,
   case-insensitive) → the `Spec:` line is rewritten to the exemption form
   with `table/diagram` or `code-not-prose`, the block is **kept** beneath
   it unchanged, and the per-case report says which exemption was chosen so
   the writer can fold the content into prose if they prefer.
4. **Already correct** (Spec first, `> "` quote next, or an exemption with
   no block) → untouched, counted as unchanged.
5. **Two or more `Spec:` lines, or two or more blockquote runs** →
   untouched and named in the report as *"N Spec lines / M blockquotes —
   normalise by hand"*. Refusing beats guessing.
6. A `Spec:` line with **no** blockquote → untouched (nothing to move; the
   advisory already covers it).

Idempotent: a second run changes nothing. Reported per case:
`normalised` / `exempted (<why>)` / `unchanged` / `by hand (<reason>)`.

### Out of scope
`update-hub.json`'s 154 notes — the user's decision; one call when ready.

## 2. `optimize_cases` stops trimming assertions (§8, §12)

`optimize.rs:352` in `clean_expected`: the cut at the first
`sentence_break` keeps everything before it and drops the rest. New rule,
applied **after** the rationale cut (which runs first and still removes
"…because that appraiser sits in {unit_b}"):

Split at sentence breaks. Keep the first sentence. For each later sentence,
keep it when it **carries an assertion**, else drop it and everything after
it. A sentence carries an assertion when either holds:
- it contains a negation: `no `, ` not `, `n't`, `never`, `neither`,
  `none`, `nothing` (word-bounded, case-insensitive); or
- it names something the kept text does not — any of: a `{placeholder}`,
  an `@param`, a `snake_case` token, a `'quoted'`/`"quoted"` value, a
  token containing a digit, or a Capitalised word not at sentence start —
  that does not already appear in the kept text.

All eight examples in §8 and §12 survive; the three genuine glosses still go.

Report: `assertions_kept: usize` alongside `expected_trimmed`, and the
tool description (`mcp.rs:187`) opens with *"Run with `dry_run: true`
first and read `expected_rewritten`"*.

## 3. Transform gaps (§10)

- `replace_in_preconditions {find, replace}` — same semantics as the other
  three (literal, replace-all, case-sensitive).
- `set_comment {value}` — sets `TestCase.comment`; empty value clears it
  (the field already serialises as absent when empty).
- For every `replace_in_*` op the report line gains the occurrence count
  (*"replaced 31 occurrence(s) in 12 case(s)"*) and, when the draft
  contains the find string in a different case anywhere the op could
  reach, a hint: *"N case(s) contain '<find>' in different capitalisation
  and were left alone"*.
- `replace_in_notes` additionally reports how many replacements landed
  **inside a `> "..."` quote** — the citation contract breaks silently
  there and the diff does not show it. Report only; no refusal.
- Tool description in `mcp.rs:206` lists the two new ops.

## 4. Copy-in keeps the newest under the obvious name (§11)

`workspace::copy_into_cases`: same name, different bytes →
1. move the existing canonical file to
   `.test-cases/.history/<stem>.<yyyymmdd-hhmmss>.json` (create the
   directory; a name clash within the same second gets `-2`);
2. write the new bytes under the canonical name.

Identical bytes still reuse the file; a file already inside the folder is
still returned as is. Nothing is ever deleted. Return type becomes
`CopiedIn { path: String, displaced: Option<String> }` (specta) so the
import toast in `ImportFile.tsx:457` can say *"Replaced cases.json — the
previous copy is in .history"*. `tests/workspace.rs:57`
(`a_different_file_with_the_same_name_gets_a_suffix_not_an_overwrite`)
inverts; a new test proves the displaced bytes are intact and the new
canonical bytes are the picked ones. `-N` suffixes are no longer produced.

Not in scope: §11.3's id-overlap warning at import — with this fix the case
it guards no longer arises.

## 5. Merge names its leftovers (§11.4)

`merge_case_files` (`ai_bridge.rs` ~1050): the response gains
`superseded: [paths]` — the slice files it consumed — and one report line
saying they are now superseded and safe to remove from `.test-cases`.
Reporting only; the tool deletes nothing.

## 6. Fanout asks for a model per slice (§13)

`ai_tools.rs:172` `COMMANDS`, the `fanout` body: add the feedback's
suggested wording verbatim (the "Choose a model per slice" paragraph, the
"State the choice" line, and the caution), placed after the dispatch
paragraph. The files on disk are regenerated by `write_commands_in`
(`commands/ai_tools.rs:486`); the plan names the user action that triggers
it.

## Testing

- Rust: every item is an integration test under `v2/src-tauri/tests/`
  (`transform.rs`, `optimize.rs`, `speccov.rs` or `ai_bridge.rs`,
  `workspace.rs`, `mcp.rs`). Each new behaviour gets a test that fails
  before the change; the §10 replace-all test and the §11 inversion are
  the two that settle a disputed claim.
- Frontend: `ImportFile.test.tsx` for the displaced-copy toast; `tsc`.
- Both full suites before the branch is finished.

---
name: test-data
description: Use when viewing or changing test data in suites/**/data/<feature>.json - "change the mark for grade 3 to 80", "edit rating methods test data", "show me the test data", "I want to edit this in Excel". The only sanctioned way to edit a data file; never hand-edit the JSON or an .xlsx directly.
---

# Editing test data

Test data lives in `suites/**/data/<feature>.json`. QA engineers own the
values but not the format, so this skill is the only front door: it shows the
data as a grid, applies changes through `cli/data/edit.mjs`, and validates.

Account credentials and identity - username, password, `employeeNo`,
`displayName`, `supervisor` - live in `src/users/users.json`, a different file
with different rules (global, not per feature; referential integrity that a
data file does not have). That file is edited with the `manage-test-users`
skill, never this one.

## Two rules that are never bent

**1. Main session only. Never delegate this to a subagent.** This skill asks
the human things - which case did you mean, is the ticket wrong, tell me when
you have finished in Excel. A subagent cannot ask anything, so it would either
hang or invent an answer. Same rule, same reason, as navigation capture in the
`/gen-test` pipeline.

**2. Visualize first, always.** The people this exists for never read the
JSON. **No path through this skill begins with a proposed change.** Render the
grid first - even when the request is completely unambiguous - because the
render is how the human confirms you landed on the right cell before anything
is written.

## The four beats

Every interaction, without exception:

```
1. RENDER    show the area, with the target marked
2. POINT     they confirm, correct, or point somewhere else
3. PROPOSE   before -> after, with any provenance consequence
4. APPLY     write JSON, then lint
```

## Beat 1 - Render

```bash
npm run data:show -- <feature>
```

Add `--case <caseKey>` to scope array sheets to one case, and
`--mark '<json>'` to call out the cell you believe they mean:

```bash
npm run data:show -- rating-methods --case createValid \
  --mark '{"sheet":"rating-methods.items","caseKey":"createValid","index":2,"header":"mark"}'
```

Run the command, then show both: the raw command output verbatim first (in a
code block, exactly as printed - this is the source of truth), followed by
the same content reformatted as a Markdown table shaped like the data - one
table per sheet (the case row, each array sheet, provenance, the legend).
Reformatting is presentation only: every case, row, column and value the
command printed must still be there, in the same order, with nothing
summarised away, elided, or reworded - and never resolve a `{{unique}}`
token, it is literal. When `--case` or a hidden-row note narrows what's
shown, say so exactly as the command did.

If the feature name is unknown or ambiguous the command says so and lists what
exists. Show that and ask; never pick.

## Beat 2 - Point

Their phrasing is not a JSON path. *"The mark for grade 3"* names a row by a
**sibling value**, which is how people talk about tables. Match what they said
against case keys, column headers **and cell values**.

- **One match** - mark it in the render, then propose.
- **Several** - list them and ask. **Never pick.**
- **None** - say so, render the grid, and invite them to point at it.

## Beat 3 - Propose

Run the mutation **without** `--apply`. It prints the change and writes
nothing:

```bash
npm run data:edit -- set <feature> '<ref-json>' --value '<text>'
```

`ref-json` is `{"sheet":..,"caseKey":..,"index":..,"header":..}` - `index` only
for an array sheet, and it is the 1-based `#` from the render.

Other mutations, same preview-then-apply shape:

```bash
npm run data:edit -- add-row    <feature> <sheet> <caseKey> '<cells-json>'
npm run data:edit -- remove-row <feature> <sheet> <caseKey> <index>
npm run data:edit -- add-field  <feature> <stem> <caseKey> <header> --value '<text>' --provenance <how>
npm run data:edit -- add-case   <feature> <stem> <caseKey> '<cells-json>' --provenance <how>
npm run data:edit -- remove-case  <feature> <stem> <caseKey>
npm run data:edit -- remove-field <feature> <stem> <caseKey> <header>
npm run data:edit -- rename-case  <feature> <stem> <oldKey> <newKey>
npm run data:edit -- provenance <feature> <stem> <caseKey> <field> <how>
```

**`remove-case` deletes data, not a test.** A case whose `test()` still calls
`loadCase(__filename, '<key>')` will fail at run time once its slice is gone,
and no linter rule catches that direction. So: removing a *test* is
`/edit-spec`, which cascades the test, its data case and its test-case section
together - `spec-editor` hands this skill the exact `remove-case` /
`remove-field` / `rename-case` commands to run. Reach for `remove-case`
directly only for a case nothing references any more, which is what
`orphan-case` reports.

`add-field` and `add-case` both require `--provenance <how>` - one of `from-ticket`,
`invented`, `assumed`, `derived`, `manual` (see "Test data provenance" in
CLAUDE.md). It is never defaulted: ask which of the five applies rather than
guessing, the same way you'd never invent the value itself. For `add-case`,
the one value you give applies to every field of the new case - the case as a
whole is invented/assumed/derived/from-ticket/manual, not any one field of it.
Omitting it, or passing something outside the five, is refused before
anything is written; the refusal names the five values.

Show the preview and ask for a yes. **Never apply without one.**

### Blank, empty and null

A blank cell means the key is absent. `<empty>` means `""`. `<null>` means
`null`. If they say "clear it", ask which one they mean rather than guessing -
a test that fills `""` and a test that omits the field do different things.
What to pass to `--value`:

| They mean | Pass | Result in the JSON |
|---|---|---|
| remove the field entirely | `--value ''` | the key is deleted |
| an empty string | `--value '<empty>'` | `"field": ""` |
| a JSON null | `--value '<null>'` | `"field": null` |

A **scalar** array row (an array of plain values, not objects) has no
"absent" - the element IS the row, so there is no key to delete. `--value ''`
on one is refused, naming `remove-row` instead: use that command to drop the
row rather than trying to blank it.

### When the cell's type cannot hold the text

A cell keeps the JSON type it already has. `"maxMarks": 50` is a number and
`"enabled": false` is a boolean, so `--value 'N/A'` and `--value 'Yes'` are
**refused**, naming the sheet, the case and the column:

```
rating-methods / a / maxMarks: this column holds numbers and "N/A" is not one.
```

This is the same refusal Excel gets from gate 6, deliberately - the two doors
must agree, or "the same edit in chat and in Excel" is not one guarantee. It is
also not a coercion you can talk it into: coercing wrote `null` for `N/A` and
`false` for `Yes`, silently.

**Read which refusal it is before relaying it.** There are two, and they mean
different things:

| The message says | What actually happened | What to tell them |
|---|---|---|
| `"N/A" is not one` | The text is not a number at all | A typo, or the field really is changing type. |
| `1.50 would be stored as 1.5` | It **is** a number, just not one that survives the round trip - `1.50`, `1e3`, `007`, `' 5'` | Not a typo. Say the value is fine but the spelling is not, and offer the exact text the message names. |
| `JSON has no way to write Infinity` | `NaN`/`Infinity` - JSON writes all of them as `null` | There is no way to store this. Ask what they meant. |
| `JSON writes them in lower case` | `TRUE`, `False` - right value, wrong casing | Just retype it lower case. |

Never tell someone `1.50` is not a number. It is one, and sending them to hunt
a typo that does not exist wastes their time.

If the field is genuinely changing type - a numeric mark becoming the text
`"N/A"` - that is structural work: it changes what the test reads, so it needs
a look at the spec, not a `set`.

The preview always shows the JSON value that will land, not the text you
typed, and calls out a type change explicitly:

```
  rating-methods.a.maxMarks
    50 -> null
    (the value's type changes: number -> null)
```

### When the value came from a ticket

If the preview says the value is `from-ticket`, the ADO test case specified
it. Do not just apply. Ask:

> This value is marked `from-ticket` - ticket #<id> specified it. Is the
> ticket wrong, or is this a deliberate test-only override?

A deliberate override is recorded, never silently - preview it first, exactly
like every other mutation in this skill, then apply once they've said yes:

```bash
npm run data:edit -- provenance <feature> <stem> <caseKey> <field> manual
npm run data:edit -- provenance <feature> <stem> <caseKey> <field> manual --apply
```

If the ticket is wrong, say so plainly and suggest updating the ticket too -
that is a human's call, not yours.

## Beat 4 - Apply

```bash
npm run data:edit -- set <feature> '<ref-json>' --value '<text>' --apply
npm run lint:tests -- --require-specs
```

Report both results. If lint fails, say so loudly and quote it - the JSON is
already written, so this is a report, not a rollback.

## Structural work only this skill can do

Excel refuses shape changes, so two of them come here, through the CLI, same
preview-then-apply shape as everything else:

```bash
npm run data:edit -- add-field <feature> <stem> <caseKey> <header> --value '<text>' --provenance <how>
npm run data:edit -- add-case  <feature> <stem> <caseKey> '<cells-json>' --provenance <how>
```

`add-field` adds one field to one case - it never touches sibling cases, so
it's the right tool for "this one case also needs a `notes` field", not the
whole sheet. `add-case` refuses a `caseKey` that already exists rather than
overwriting it. Both require `--provenance <how>` - see Beat 3 above.

When adding a **case key**, first check a matching `test()` exists in the
spec. The data file's own path - printed at the top of every `data:show`
render - tells you where: swap `data/<feature>.json` for
`specs/<stem>.spec.ts` in that same path and grep it, e.g.

```bash
grep -n "loadCase" "suites/sl/admin/performance/DW-rating-methods/specs/rating-methods.spec.ts"
```

Do not glob for the spec (`suites/**/specs/...`) - `**` is not recursive in
every shell this skill runs under, and a pattern that silently matches
nothing is worse than one that visibly fails. The exact path from the render
always works. A case with no test is invisible dead data. If there is none,
say so and ask whether to add it anyway.

### What is not automated

These are not wired into `cli/data/edit.mjs` today. Say so and stop - do not
work around the gap (see "Never" below):

| Request | What to do |
|---|---|
| add a whole new spec stem (a new top-level key in the JSON) | say it isn't automated yet, report it |
| repair a shape `data:show` refuses (`array-of-arrays`, `mixed-array`, etc.) | say it isn't automated yet, report it - see the refusals section below for what the fix would look like |

**Do not work around a missing command by editing the JSON directly.** A gap
in the tooling is a thing to report, not a licence to hand-edit - the whole
point of this skill is that no edit reaches a data file except through
`cli/data/edit.mjs`.

## When the file cannot be shown as a grid

`data:show` lists refusals instead of a grid - `array-of-arrays`,
`mixed-array`, `mixed-stem`, `nested-array`, `country-delta`,
`reserved-field-name`, and `mixed-column-type`. Every mutation is blocked
until the shape is repaired. Show the refusals and explain the fix in the
message - `mixed-stem` is the common one and its fix is moving loose values
into a `_shared` block, `reserved-field-name` (a field literally named
`case`, `#`, or starting with `prov:` - the grid's own scaffolding) is fixed
by renaming the field, and `mixed-column-type` (an array column whose rows
disagree on JSON type, e.g. one case's `mark` is `"75"` and another's is
`90`) is fixed by making every row's value for that field the same type -
but there is no command for any of these (see "What is not automated"
above): say so and report it rather than making the change yourself.

## Never

- Never hand-edit a `data/*.json` with Write or Edit. Every change goes
  through `cli/data/edit.mjs`, which is what keeps a conversational edit and
  an Excel edit producing identical JSON.
- Never edit an `.xlsx` yourself.
- Never remove or resolve a `{{unique}}` token.
- Never apply a change without showing the preview and getting a yes.

## Survey disk before doing anything

State lives in the workbook on disk, **never in this conversation**. A QA
engineer may spend an hour in a spreadsheet, close their laptop, and come back
tomorrow to a session that remembers nothing. So on every invocation, before
doing anything else for a feature that might have a workbook, run the safe
probe:

```bash
npm run data:sync -- <feature>
```

With no `--apply` it writes nothing - it only reads the JSON and, if one
exists, the workbook - and its output tells you exactly which situation you
are in:

| It prints | Meaning | What to do |
|---|---|---|
| `No workbook for "<feature>". Ask for a spreadsheet first.` | Clean, no `.xlsx` yet | Go straight to the four beats above |
| `No changes - the workbook matches the JSON.` | Exported but untouched | Go through the four beats above, then refresh the workbook if one exists (see "Keeping the workbook in sync") |
| `<N> change(s). Nothing written. Re-run with --apply.` | **Unsynced edits waiting** | Show the preview verbatim; offer to sync those first |
| `[gate 3] stale-workbook ...` | **Workbook is stale** - the JSON changed after it was exported | Offer a re-export; warn it discards their edits |
| `<feature>.xlsx is open in Excel. Save, close it, then ask again.` | Open in Excel right now | Stop. Ask them to close it |

Do not try to work this out with `ls` on the feature's `data/` directory. A
`**` glob is not recursive in every shell this skill runs under (the same
trap "Do not glob for the spec" above warns about), and `data:sync` already
does the check correctly by reading the files itself - it is the one source
of truth for what state a feature's data is in.

## The Excel handoff

Render the grid first (beat 1), so they know what they are about to open.
Then:

```bash
npm run data:excel -- <feature>
```

Tell them where it is and hand over:

> Your spreadsheet is ready at `<path>`.
> Grey columns are locked - the tooling owns them. Don't remove a `{{unique}}`
> token; it stops two tests colliding on the same record name.
> On an array sheet you can add, delete and reorder rows freely. Add one
> directly under a row of the case it belongs to - the grey `case` and `#`
> cells stay blank and are filled in for you from where the row sits.
> Because a case's block is identified by position, a row inserted directly
> *above* a case's first row joins the case above it, not the one you were
> aiming at. A case whose array is empty has no rows on the sheet at all, so
> there is nowhere to put a row for it - a row meant for it joins whichever
> case comes before it; ask me to add that first row instead.
> Open it, make your changes, **save and close it**, then tell me you're done.

**Then stop and wait.** Do not poll, do not guess that they have finished, and
do not start anything else. This is the same wait as navigation capture in
`/gen-test`.

When they say they are done:

```bash
npm run data:sync -- <feature>
```

Show the output verbatim, then ask for a yes before:

```bash
npm run data:sync -- <feature> --apply
npm run lint:tests -- --require-specs
```

The workbook is rebuilt and re-stamped automatically, so they can keep working
in the same file for another round.

### When sync refuses

It names the gate and writes nothing. Explain it plainly - never work around
it:

| Gate | What happened | What to tell them |
|---|---|---|
| 3 | The JSON changed after they exported | Their spreadsheet is out of date. Re-export and redo the edits. Say what changed underneath them. |
| 4 | A column was added, renamed or deleted | Structural work. Offer to do it here instead. |
| 5 | A case row was added or removed | Same - and check a `test()` exists before adding a case. |
| 6 | Excel coerced a value, or the typed text is not the type the cell holds | Usually a number or a date - re-export and retype it as text. If instead they typed something the cell's type cannot hold (`Yes` in a true/false column), it is the same refusal `set` gives; see "When the cell's type cannot hold the text". |
| 7 | A `{{unique}}` token was removed | Explain why it exists, and put it back. |

One refusal happens before the gates, while the workbook is still being read:
an array row whose `case` cell is blank and which has **no array row above
it**. A new row takes its case from the row directly above, so one placed
above every existing row has nothing to take one from. Tell them to move it
below a row of the case it belongs to - not to type a case in, which the
locked column exists to prevent.

**Never** hand-edit the JSON to "fix" a refused sync. The refusal is the
feature working.

### When sync partially fails

The JSON write and the workbook rebuild are two separate steps inside
`--apply`, and only the second can fail on its own - typically because the
workbook got reopened in Excel in the moment between the safe probe and the
apply. When that happens, `data:sync --apply` prints something like:

```
Your changes ARE saved in <feature>.json.
Only the workbook rebuild failed: <reason>
The spreadsheet is now out of date. Close it and re-export:
  npm run data:excel -- <feature>
```

and exits non-zero. **This is not a failed sync - their change already
landed in the JSON.** Do not tell them the sync failed, and do not treat the
non-zero exit as a reason to re-run `--apply` - the workbook it would rebuild
against is already stale. Tell them plainly that their edit is saved and that
only the spreadsheet file itself needs a refresh, then re-export a current one
for them:

```bash
npm run data:excel -- <feature>
```

## The collision - a chat edit while the workbook has unsynced changes

If they ask for a conversational change and `data:sync` reports pending
changes, refreshing the workbook would destroy those. Do not choose. Ask:

> The spreadsheet has changes you haven't synced yet:
>   `<the sync preview>`
> You're now asking me to change:
>   `<the proposed edit>`
> Applying mine would throw yours away. How do you want to proceed?
>   1. Sync the spreadsheet first, then apply your change  (nothing lost)
>   2. Discard the spreadsheet change and apply yours
>   3. Cancel

For option 2, re-export **after** applying the JSON edit - that discards the
workbook's pending edits by replacing the file.

## Keeping the workbook in sync

Whenever you write the JSON and a workbook exists, refresh it in the same
breath, so it is never stale:

```bash
npm run data:edit -- set <feature> '<ref>' --value '<text>' --apply
npm run data:excel -- <feature>     # only if a workbook exists
npm run lint:tests -- --require-specs
```

If the workbook is open in Excel right now, `data:excel` refuses on its own
(`<feature>.xlsx is open in Excel. Close it first, then ask again.`) rather
than writing over unsaved work - stop there, ask them to close it, then re-run
the same `data:excel` command once they have.

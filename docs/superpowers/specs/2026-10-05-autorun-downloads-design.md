# Auto Run download checks (part 3 of 5)

Design agreed with the owner on 2026-10-05, from item 5 of the backlog in
`docs/autorun/backlog-2026-10.md`. It unblocks case 137540 (the template's
columns) and lets case 137537 check the error log's messages, not only the
response.

Owner decisions:
- A downloaded file is kept with its run on this machine. It is never sent
  to Azure DevOps.
- Ships as its own beta.

## 1. Capture

Every Auto Run browser saves its downloads instead of discarding them. That
covers unattended runs, the supervised browser and the assistant's tries.

- **How:** when the browser is attached, the driver sends
  `Browser.setDownloadBehavior` with `behavior: "allowAndName"`, a download
  folder, and `eventsEnabled: true`. It then follows `Browser.downloadWillBegin`
  (the guid and the suggested file name) and `Browser.downloadProgress` (the
  guid and the state `inProgress`, `completed` or `canceled`). This lives in
  the CDP driver, beside the request handling (`browser/cdp.rs`), so events
  are read during every call.
- **Where:**
  - unattended runs: `downloads/<run id>/` beside the run's screenshots;
  - the supervised browser: `downloads/supervised/`, emptied when the browser
    closes.
  A file is saved under its guid by the browser. The driver renames it to the
  suggested name, adding ` (2)` and so on when two downloads in one run share
  a name. Names are sanitised to a plain file name: no path parts, and
  `\ / : * ? " < > |` are replaced.
- **What the driver keeps:** a list per browser of
  `{ guid, name, path, started_at, state, bytes }`, in start order.

## 2. The `expect_download` action

```json
{ "kind": "expect_download",
  "name": "Template*.xlsx",
  "within_ms": 15000,
  "sheet": "Employees",
  "headers": { "exact": ["Employee No", "Name", "Department"] },
  "cells": [ { "ref": "B2", "text": "Employee Name", "match": "exact" } ],
  "contains_text": ["Row 4: Department is required"] }
```

- **Which download it checks:** it waits up to `within_ms` (default 15000,
  most 120000) for the first download that started during this step. It
  checks that one once it completes. A download that started in an earlier
  step is not this step's.
- **`name`:** required. It is matched against the whole file name,
  case-insensitively, either exactly or as a pattern where `*` stands for any
  run of characters.
- **Spreadsheet checks, for `.xlsx`, `.xls` and `.csv` only:**
  - `sheet`: the sheet by name. If omitted, the first sheet. A CSV has one
    sheet and ignores `sheet`.
  - `headers`: the first row, as `{ "exact": [...] }` (exactly these, in this
    order, trimmed) or `{ "contains": [...] }` (each present in any order).
  - `cells`: an A1-style `ref`, the `text` to find, and `match` (`exact`, the
    default, or `contains`). The comparison is on the cell's displayed text,
    trimmed.
- **`contains_text`, for `.csv` and `.txt`:** each string must appear in the
  file's text.
- **Validation at save:** `name` is required, and at least `name` is
  checked. Spreadsheet keys on a file type that cannot carry them, `within_ms`
  out of range, an invalid `ref` and empty lists are all refused, each with a
  sentence naming the key.
- **It is a check:** it counts toward the expected-result floor, like the
  other `expect_*` actions.
- **Outcomes**, all plain sentences:
  - Passed: `downloaded "<name>" (<size>)`, plus one clause per check, for
    example `headers match`, `B2 is "Employee Name"`.
  - `no download started within <n> s`
  - `the download "<name>" was canceled`, or `did not finish within <n> s`
  - `got "<actual>", expected a file named "<pattern>"`
  - `sheet "<sheet>" is not in "<name>" (it has: <sheets>)`
  - `headers are <actual>, expected <expected>`
  - `<ref> is "<actual>", expected "<text>"` (or `expected it to contain "<text>"`)
  - `"<name>" does not contain "<text>"`
  - `"<name>" is <size>, over the 50 MB that can be checked`
  - `"<name>" could not be read as a spreadsheet: <reason>`

  An outcome never prints more than 200 characters of cell or file text.
- **Reading:**
  - xlsx and xls: the `calamine` crate (read only, pure Rust);
  - csv: the `csv` crate, with the delimiter detected from the first line
    (comma, semicolon or tab), and UTF-8 with or without a BOM, falling back
    to Windows-1252;
  - txt: UTF-8, falling back to Windows-1252.

## 3. Where the files are seen

- **Past runs and the review:** a case with downloads lists them under its
  steps, each with its name, size and an Open button. Open hands the file to
  the default app; only files inside the run's own download folder can be
  opened.
- **The HTML report:** a `Downloads:` line per case, naming each file and its
  size (no link).
- **Sending to Azure DevOps** sends no download.
- **Clearing a run** removes its download folder.
- **The assistant** (`get_autorun_failures`) sees the outcome sentences and
  the file names, never the file contents.

## 4. The guide

The Auto Run guide (`autorun/guide.rs`) documents the action and its keys,
with two examples:
- 137540: a template's columns checked with `headers.exact`;
- 137537: an error log's messages checked with `contains_text` or `cells`.

It also says to click the export in the same step, just before
`expect_download`: only a download that starts during the step is the
step's (ruled 2026-10-05, matching how an Azure DevOps step holds both the
action and its expected result). Supervised runs list no downloads, since
their folder is emptied when the browser closes.

## 5. Out of scope

- Uploading downloaded files elsewhere, comparing two downloads, and PDFs
  or Word files (only their names can be checked).
- Downloads in the How To Use guide's sample data (the screenshots are not
  retaken in this part).

## 6. Testing

- Rust tests in `tests/suite` only.
- The pure parts are tested without a browser: name patterns, sanitising,
  the A1 parse, the header and cell checks, the CSV delimiter and encodings,
  the size cap, and every outcome sentence. Small xlsx, xls, csv and txt
  fixtures are committed under `tests/fixtures/downloads/`.
- **Live test (headless Edge):** a fixture page whose button downloads an
  xlsx and a csv. `expect_download` passes for each with headers and cells,
  fails on a wrong name, and reports "no download started" when nothing is
  clicked.
- **vitest:** the downloads list in the review and Past runs, and Open.

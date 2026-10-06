# Auto Run: browser dialogs, table checks, page errors, PDF downloads, drag and key combinations

Design agreed with the owner on 2026-10-06. It covers backlog items 14 to 17.

## Owner decisions

- **Dialogs.** A script can do three things with a browser dialog:
  - check its text, then accept or dismiss it;
  - type into a prompt;
  - opt in to failing on a dialog that no step expected.
- **Tables.** Real HTML tables and ARIA grids (`role=grid`, `row` and `cell`) are both understood. Columns are found by their header text.
- **Page errors.** A script opts in and chooses one of two modes:
  - fail the step;
  - flag the case.

  It is off by default.
- **PDF downloads.** Three checks: the PDF contains given text, its page count, and text on a given page.

Every new field is serialised only when set, so old scripts and old run files load unchanged. Every sentence below is used verbatim. The readable script view (`describeAction.ts`) gets a sentence for each new action.

A failure sentence below that can have a cause carries it at its end, as `: <reason>` (ruled 2026-10-07).

## 1. Browser dialogs

### Today

Every `alert`, `confirm`, `prompt` and `beforeunload` dialog is accepted automatically, in every tab (`browser/cdp.rs`, `Page.javascriptDialogOpening`).

### The new action

`expect_dialog { text?, contains?, answer: "accept" | "dismiss", prompt_text?, within_ms? }`

- **When it arms.** Every `expect_dialog` in a step is armed when the step starts, so a dialog that a click earlier in the same step opens is caught.
- **What it waits for.** The next dialog in any tab of the run. It waits up to `within_ms`, 10 s by default, after the step's other actions have finished.
- **Checking the text.**
  - `text` must equal the dialog's message after trimming.
  - `contains` must appear in it, ignoring case.
  - A script may give either or neither, never both: `expect_dialog takes text or contains, not both`.
- **Answering.** `accept` presses OK, and `dismiss` presses Cancel.
- **Prompts.** `prompt_text` is typed into a `prompt` before OK. It is refused with `prompt_text needs answer accept` when `answer` is `dismiss`.
- **Failure sentences:**
  - `no dialog appeared within <n> seconds`
  - `the dialog said "<message>", not "<text>"`
  - `the dialog said "<message>", which does not contain "<text>"`
- **When the text check fails.** The dialog is still answered as the step asked, so the page is not left stuck, and the step fails.
- **Recording.** The step record holds the dialog's kind (`alert`, `confirm`, `prompt`, `beforeunload`) and its message.

### Unexpected dialogs

- **The option.** A script can set `fail_on_unexpected_dialog: true`.
- **What it does.** A dialog that no armed `expect_dialog` claims is still accepted, as today, so the page can go on. The step that was running then fails with `an unexpected <kind> dialog appeared: "<message>"`.
- **Without the option.** Behaviour is unchanged. The dialog is accepted, and it is noted in the step's log as `a <kind> dialog was accepted: "<message>"`.
- **No secrets.** A message is cut to 200 characters in sentences and records. It is never treated as a secret, because dialogs are page text.

## 2. Table and grid checks

### Finding the table

- **The `table` locator.** Each check takes a `table` locator, the same kinds every other locator accepts: role and name, text, CSS, or a frame chain. It must find either:
  - a `<table>`;
  - an element with `role=grid`, `role=treegrid` or `role=table`.
- **Rows and cells.**
  - In an HTML table, rows are `<tr>` in `<tbody>`, or all `<tr>` except the header row. Cells are `<td>` and `<th>`.
  - In an ARIA grid, rows are `role=row` that are not header rows. Cells are `role=gridcell`, `role=cell` and `role=rowheader`.
- **Headers.** Header texts come from `<th>` in `<thead>` or the first row, or from `role=columnheader`.
- **Matching columns.** Columns are matched by header text, trimmed and with case ignored. An unknown column fails with `the table has no column "<name>" - its columns are <list>`.
- **Cell text.** This is the cell's visible text, trimmed, with inner spaces collapsed.

### The actions

- **`expect_row { table, cells: { "<column>": "<text>" }, exact? }`**
  - Passes when some row has every listed cell. Matching ignores case. By default each text is contained in the cell; with `exact: true` it must equal the cell.
  - Failure: `no row has <column> "<text>"[, <column> "<text>"]` followed by ` - the table has <n> rows`.
- **`expect_no_row { table, cells, exact? }`**
  - The reverse. Failure: `a row has <cells>` (row <n>).
- **`expect_sorted { table, column, order: "ascending" | "descending", as?: "text" | "number" | "date" }`**
  - Checks that the column's values are in order. Blank cells are ignored.
  - `as` defaults to `text`, which compares ignoring case.
  - `number` reads digits with `,` thousands separators and a leading `-`.
  - `date` reads the formats `yyyy-MM-dd`, `dd/MM/yyyy`, `MM/dd/yyyy` and `d MMM yyyy`. When `dd/MM` and `MM/dd` are ambiguous, the step fails with `the dates in <column> could be read two ways - give as: "date" a format`. In that case `as` may be `{ "date": "dd/MM/yyyy" }`.
  - Failure: `<column> is not in <order> order - row <n> "<a>" comes before row <n+1> "<b>"`.
- **`expect_row_count { table, equals? | at_least? | at_most? }`**
  - Exactly one of the three is given.
  - Failure: `the table has <n> rows, not <expected>`, with "at least" or "at most" wording when those are used.

### Timing

All four checks retry for up to the step's check timeout, the same as `expect_text`, so a grid that is still loading is waited for.

### Scope

- The checks read only the rows the page has rendered.
- A grid that shows rows page by page, or loads them while scrolling, is checked as shown. The guide says to filter or page first.

## 3. Page errors as a check

### The option

A script can set `page_errors: "fail" | "flag"`. It is absent by default, which keeps today's behaviour.

### What counts during a step

- An uncaught JavaScript error (`Runtime.exceptionThrown`) in any tab of the run.
- A request answered with a 5xx status, from any tab.
- Requests made by the run itself are excluded: `api_request`, the sign-in, and setup fixtures.
- Errors that happen between steps are counted against the next step.

### The two modes

- **`fail`.** The step fails with one of:
  - `the page had an error: <message>`, for a script error, cut to 200 characters with no URL query;
  - `a request was answered <status>: <method> <path>`, path only.

  When several errors happen in one step, the first is named and the rest are counted: `(and <n> more)`.
- **`flag`.** The step is judged as usual. The case then carries `page errors seen: <n>`, shown as a badge in the review, in Past runs and in the report. Each error is listed in the step's log.

### Ignoring known noise

`ignore_page_errors: [<text>]` holds up to 10 phrases. An error whose message, or whose request path, contains one of them (ignoring case) is not counted. The phrases apply first: `(and <n> more)` and `page errors seen` count only the errors left after them.

## 4. PDF downloads

`expect_download` gains a `pdf` block:

`pdf: { contains?: [<text>], pages?: { equals? | at_least? | at_most? }, on_page?: [{ page, contains }] }`

- **Reading.** The PDF's text is extracted in Rust with a PDF text crate. The plan picks one that can give text page by page.
- **Comparing.** Text is compared ignoring case, with all runs of whitespace treated as one space.
- **`page`.** It is 1-based. `-1` means the last page.
- **Failure sentences:**
  - `the PDF does not contain "<text>"`
  - `the PDF has <n> pages, not <expected>`
  - `page <p> of the PDF does not contain "<text>"`
  - `the PDF has no page <p>`
  - `the PDF's text could not be read`, for an encrypted file, a scanned image with no text, or a damaged file. The detail is written to the app log.
- **Using the block.** A `pdf` block is allowed only when the download's name pattern ends in `.pdf`, otherwise `pdf checks need a name ending in .pdf`. It joins the existing headers, cell and text checks, which are for spreadsheets, CSV and text files.
- **Limits.** Files over 50 MB are refused with `the PDF is larger than 50 MB`. Memory is bounded only by that 50 MB input: compressed streams inside a PDF can expand beyond it. This is a known limit.
- **Read time.** Reading a downloaded file's content takes at most 60 seconds. Past that, the step fails with `"<name>" took longer than 60 seconds to read`, naming the file only. The run's Stop ends the wait at once. The abandoned read is left to finish on its own.

Rulings made during the build (2026-10-07):
- **One page.** The page-count sentence is singular for one page: `the PDF has 1 page, not 3`.
- **`-1` in a failure.** A failed `on_page` with `-1` names the real page number: `page 2 of the PDF does not contain "<text>"`.
- **Page breaks.** The top-level `contains` matches across page breaks: the pages are joined with one space before it is checked.
- **Owner passwords.** A PDF locked only with an owner password, against changes, opens with no password and is read. Only a PDF that needs a password to open counts as unreadable.

## 5. Drag to reorder, and key combinations

Added by the owner on 2026-10-06. Auto Run has no drag action today. `press_key` sends only a single key, so a script cannot use the keyboard alternative to dragging either, such as Ctrl+Arrow.

### Key combinations

`press_key` accepts modifiers joined with `+`:
- **The modifiers** are `Ctrl`, `Shift`, `Alt` and `Meta`, ignoring case.
- **The key** is any key name it accepts today, for example `Ctrl+ArrowUp`, `Shift+Tab`, `Ctrl+Shift+End` or `Alt+ArrowDown`.

The modifiers are held down for the key press, in the order Ctrl, Alt, Shift, Meta, and released in reverse. They use the CDP modifier bitmask on every key event.

`press_key` also gains `times` (1 to 50, default 1), which presses the same combination that many times.

**Refusals at save:**
- an unknown modifier: `press_key: "<part>" is not a modifier - use Ctrl, Shift, Alt or Meta`;
- a combination with no key: `press_key: "<value>" has no key after its modifiers`;
- a modifier given twice: `press_key: "<modifier>" is given twice`.

**Old scripts** with a single key are unchanged.

### Drag

`drag { from, to, position?: "before" | "after" | "onto", within_ms? }`

- **What it does.** `from` is the element to pick up, and `to` the element to drop it on. Both are ordinary locators, frame chains included. `position` says where on `to` to drop, and defaults to `onto`:
  - `before` drops on the upper part of `to`;
  - `after` drops on the lower part;
  - `onto` drops in the middle.
- **Making both visible.** Both elements are scrolled into view first.
- **The mouse path.** This serves pages that drag with mouse or pointer events, such as most sortable lists and grid row reordering:
  1. press on `from`'s centre;
  2. move 10 px to start the drag;
  3. move in 10 steps to the drop point, waiting one animation frame between moves;
  4. release.
- **The HTML drag-and-drop path.** This serves pages that use the browser's own `draggable` drag and drop.
  - The driver turns on drag interception (`Input.setInterceptDrags`) for the move.
  - When the browser hands over a drag (`Input.dragIntercepted`), the driver completes it with `Input.dispatchDragEvent` at the drop point: `dragEnter`, then `dragOver`, then `drop`.
  - Interception is turned off again afterwards on every path.
- **Failure sentences:**
  - `there was nothing to drag at <from>`, when `from` is not found or not visible;
  - `there was nowhere to drop at <to>`;
  - `the drag did not finish within <n> seconds`.

  An action does not check its own effect, so a drag that the page ignored passes the action. The guide tells the script to follow a drag with a check of the new order, for example `expect_row` or `expect_text`.
- **The must-not-save guard is unchanged.** A drag is an action, so a page that saves on drop is stopped under "must not save" like any other save.
- **The readable sentence:** `Drag <from> before|after|onto <to>`.
- **The guide** explains drag and the keyboard alternative. It names Ctrl+Arrow and Alt+Arrow as common reorder keys, and says to use the one the screen documents.

## 6. Everywhere

- **Assistant's guide.** `autorun/guide.rs` explains each new action and option, with one example each. It also says when to use `flag` rather than `fail`.
- **Readable script and reports.** `describeAction.ts`, `report::action_words`, `patterns.rs` and `ai_bridge::describe_try` get words for every new action.
- **How To Use guide.** It gets one short tip per feature in the scripts section, with no new screenshots.

## 7. Out of scope

- Comparing whole tables, and tables spread across pages.
- OCR of scanned PDFs.
- Changing what an unexpected dialog does beyond accept-or-fail.
- Page errors from the app's own background polling when it runs between cases.

## 8. Testing

### Rust

- **Drag and keys:**
  - each modifier and combination, with the bitmask on every event, and `times`;
  - each refusal sentence;
  - the drag mouse path (event order and positions for before, after and onto);
  - the HTML drag path (interception on, `dispatchDragEvent` order, interception off on every path, including failure);
  - each drag failure sentence;
  - a drag in a second tab and in a frame.

Tests live in `tests/suite` only, with fake pages:

- **Dialogs:**
  - each dialog kind;
  - accept, dismiss and prompt text;
  - a dialog opened by a click in the same step;
  - a dialog in a second tab;
  - each failure sentence;
  - an unexpected dialog with and without the option.
- **Tables:**
  - an HTML table and an ARIA grid;
  - header lookup and an unknown column;
  - contains and exact matching;
  - sorting as text, number and date, including the ambiguous date case;
  - each row-count form;
  - retry while the grid loads.
- **Page errors:**
  - a script error and a 5xx in fail and flag modes;
  - the run's own requests excluded;
  - ignore phrases;
  - errors between steps counted against the next step;
  - several errors in one step;
  - the flag recorded in the run file.
- **PDFs:**
  - contains, page count and `on_page`, including `-1`;
  - an encrypted or unreadable file;
  - the size limit;
  - a `pdf` block on a non-PDF name refused.
- **Compatibility:** old scripts and old run files round-trip unchanged.

### Live (headless Edge)

A fixture page that:
- opens a `confirm`, checked and dismissed;
- has a sortable HTML table and an ARIA grid;
- throws a script error;
- downloads a small PDF;
- has a SortableJS-style mouse-sorted list and a native `draggable` list, each reordered by `drag` and by Ctrl+Arrow keys.

### vitest

- The new describer sentences.
- The "page errors seen" badge in the review and in Past runs.

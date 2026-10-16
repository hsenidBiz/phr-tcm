# Crystal Reports module - design

Date: 2026-10-16. Status: draft for the owner's review.

## Why

Building and changing the EHRM's Crystal Reports by hand is slow, and formatting is the worst
of it: fonts, positions, sizes, alignment, sections, page setup. When an AI assistant did that
work from plain requests, it saved hours or days. The other half of a report is its data. The
assistant writes or changes the stored procedure behind a report on the Dev database, runs it,
and checks that the report shows the right rows for the business rule.

TCM already has the pieces around this: an MCP server that AI assistants call, database access
with a read-only/Dev-login split, and optional modules. The owner has written a pure-Rust
re-implementation of what Crystal Reports does with `.rpt` files ("the bundle", at
`Desktop\Main Menu\Software\Personal Projects\CrystalReports ReImplementation\Out`). This
module puts the bundle behind TCM's MCP server and adds a screen to watch the result.

Success: the owner asks an assistant to reformat a report, or to change its data, and gets a
saved `.rpt` that opens in Crystal Reports and shows the right rows, without touching the
designer until the final check.

## Goals

1. A **Crystal Reports** module in Settings → Modules, off by default. While it is off, every
   tool and the screen below are refused or hidden, exactly like the other modules.
2. MCP tools that let an assistant open a `.rpt` by path, read it, edit it, save it, compare
   two reports, check a report will load, create a blank report and preview it as a PDF.
3. On the **Dev login only**, the assistant may create and alter stored procedures, views and
   functions, so it can build a report's data and test it.
4. A **Crystal Reports screen** in TCM showing the report the assistant is working on as a PDF
   preview, with an **Open in Crystal Reports** button for the final check in the designer.

## Non-goals

- A full report editor in TCM. The assistant edits; the screen is for watching and checking.
  The bundle's `ui-react` editor components are not brought in.
- Anything the bundle cannot do yet. Today that is: text objects with embedded fields
  (read-only), subreport internals, paper size or orientation on a report saved without a
  printer, password-protected reports, pre-XI files. The tools say so instead of trying (the
  bundle refuses these itself; TCM passes its message on).
- Backups or undo inside TCM. The reports live in a git repo; git is the undo. Saving over the
  opened file is fine (owner, 2026-10-16).
- Dropping or creating tables, indexes, triggers, logins or anything else in the database.

## Anti-goals (a failure even if it works)

- A saved `.rpt` that Crystal Reports will not open. Every write goes through the bundle's
  verified writer, which reopens and reads back the file before reporting success; TCM never
  writes `.rpt` bytes any other way.
- A schema change reaching any database other than the Dev login's, or a `DROP` anywhere.
- A connection string, password or host appearing in a tool reply, the log or the webview.
- TCM's copy of the bundle changing without the owner saying so (see Constraints).

## Constraints

- **The bundle is a copy, refreshed only on the owner's word.** It is still in active
  development. TCM vendors a snapshot into this repository (it is pushed to GitHub, so a path
  to a desktop folder would not build there). The copy records where and when it came from.
  *(My call)* The copy is the `Out` folder file for file, leaving out only its `target\` and
  `work\` folders; no file inside it is changed, its root `Cargo.toml` included, so its crates
  keep inheriting their own workspace tables and a refresh is a plain re-copy. TCM depends on
  `rpt-core` (and `rpt-db-mssql`) by path and calls them through its own specta-typed
  commands; the bundle's `tauri-commands` glue is not used. If the copied test fixtures turn
  out too large for the repo, the builder stops and asks the owner rather than trimming.
  The original folder is read-only to us: read its docs and samples, run nothing in it, no git.
  When TCM needs something the bundle lacks, we tell the owner; we do not patch the bundle.
- **Licence.** The vendored rpt-rs crates are MPL-2.0. Their licence files,
  `THIRD-PARTY-NOTICES.md`, `VENDORED.md` and the bundled fonts' licence texts travel with the
  copy unchanged.
- **One build cache** (`src-tauri/target`), as everywhere in this repo. The bundle's own
  `target/` folder is never copied.
- The repo's usual rules hold: Rust tests only in `src-tauri/tests/suite/`, `src/bindings.ts`
  generated, no hardcoded colours, user-facing errors name no URL or host, no HTTP DELETE.

## How it works

### The module

`Module::CrystalReports` joins `modules.rs` (key, label, description in the Modules screen's
copy). It gates the new MCP tools (refused with the standard module-off sentence), the new
screen's sidebar entry, and the new Tauri commands.

*(My call)* It sits directly under Core in the module tree, not under Database Access, so
report formatting works with no database at all. The two database parts - a live-data preview
and the create/alter allowance below - need **both** Crystal Reports and Database Access on;
with Crystal Reports off, create/alter is refused exactly as today even on the Dev login.

**Which files** (owner, 2026-10-16): any `.rpt` path the person's Windows account can write.
TCM does not limit reports to a folder or to a git repository.

### The open report

The app holds one open report at a time (path, the bundle's in-memory state and IR). Opening
another replaces it. Tools act on it. The Crystal Reports screen shows it.

**Edits save immediately** *(my call)*. A batch that the bundle accepts is written straight
over the opened file, through the bundle's save path, before the tool answers. So the file on
disk, the preview and what the assistant just did always agree, and Open in Crystal Reports
never shows something older. Git is the undo. A batch the bundle refuses saves nothing that
the bundle did not already apply: the bundle may have applied the batch's earlier ops in
memory, so on a refusal TCM reloads the report from disk and answers with the refusal and the
fresh IR (the bundle's own "re-request after an error" rule).

### MCP tools

All are module-gated, run inside the app, and take or return JSON. Names are the builder's to
settle in the existing naming style; the set is:

| tool | does |
|---|---|
| open report | open a `.rpt` by absolute path; returns the IR (the bundle's `crpt-ir/1`) |
| read report | the current IR, optionally only one section or object, so a large report does not flood the assistant |
| edit report | apply native edit ops (`rpt-core` `EditOp`) or a Vibey layout plan; saves; returns the refreshed IR or the refusal |
| compare reports | the bundle's semantic diff between two `.rpt` files |
| check report | the bundle's offline load-path validation of a `.rpt` |
| new report | create a blank `.rpt` at a path (refused if a file is there) and open it |
| preview report | render the open report to PDF for the screen; returns page count and the renderer's diagnostics, never the PDF bytes |
| report guide | how to work with reports: units (twips, points, `#RRGGBB`), the op vocabulary, the limits above, the open/edit/preview/check loop, and the database rules below |

The guide is written in the same voice as TCM's other guides and is what tells the assistant
the bundle's limits, so it says "that is not supported yet" instead of trying.

### Preview and data

- **Saved data:** the bundle renders from the data saved in the report when it has some;
  otherwise the static bands.
- **Live data:** the bundle's `rpt-db-mssql` runs the report's stored procedure through TCM's
  current database connection and renders fresh rows. The connection string stays in Rust:
  it is handed to the bundle and never returned, logged or sent to the webview (the bundle
  already parses it once and never logs it).
- **Which connections may run a live preview** *(my call)*: running a report's stored
  procedure is a procedure call, and TCM's existing rule treats a call to any non-lookup
  procedure as a write (Dev login, writes switched on). Live preview follows that same rule.
  It is a **separate tool** from the saved-data preview, so it carries the same per-call
  approval as other database writes (and `db_auto_approve` applies to it the same way). The
  report's procedure name is checked the way `classify_exec` checks a call before anything
  runs: a database-qualified name is accepted only when the database is the connection's own
  (the qualifier is then dropped); another database, a server name, or an `sp_`/`xp_`
  procedure is refused. On any other connection the live tool refuses with the read-only
  sentence; the saved-data preview still works.
- Report parameters for a live preview come in the preview tool's call (`name = value` pairs);
  a missing required parameter is refused with its name.

### Database: create and alter on the Dev login

`db/guard.rs` gains one allowance, on `Access::DevWrites` with writes switched on, through
the same query path, approval behaviour and logging as today's writes:

- `CREATE`, `ALTER` or `CREATE OR ALTER` of a `PROCEDURE`, `VIEW` or `FUNCTION`, one object
  per statement, in the connection's own database (no database- or server-qualified name).
- Every other connection: refused as today, with the read-only sentence.
- Still refused everywhere: `DROP`, `TRUNCATE`, and creating or altering anything else (tables,
  indexes, triggers, schemas, logins, users).
- The object's body is checked too, because it runs later. It is classified by its own rule,
  not the single-statement one, since a body is many statements. Allowed in a body: reads,
  the four write verbs, the ordinary T-SQL a body needs (`DECLARE`, `SET`, `IF`/`ELSE`,
  `WHILE`, `BEGIN`/`END`, `RETURN`, temp tables and table variables, `TRY`/`CATCH`,
  transactions, cursors, `EXEC` of another procedure in the same database) and, on the
  owner's decision (2026-10-16), **dynamic SQL** (`sp_executesql`, `EXEC(@sql)`): existing
  report procedures use it. The guard cannot see inside a dynamic string; that is accepted on
  the Dev database. Still refused inside a body, wherever they appear outside a string:
  `xp_cmdshell` and other `xp_` procedures, `OPENROWSET`/`OPENQUERY`/`OPENDATASOURCE`,
  linked-server or other-database names, `GRANT`/`REVOKE`/`DENY`, `BACKUP`/`RESTORE`, `KILL`,
  `SHUTDOWN`, `RECONFIGURE`, `DBCC`, `EXECUTE AS`/`SETUSER`/`REVERT`, and `DROP`/`TRUNCATE`
  of anything other than a temp table (`#name`).
- Reading an existing object's definition (to change it) is already a read.

### The Crystal Reports screen

A sidebar entry while the module is on.

- Header: the open report's file name and folder, or "No report open" with a line saying the
  assistant opens one.
- The PDF preview of the open report, refreshed after each edit or preview call (an app event
  tells the screen; it reads the PDF through a Tauri command). Page count and any renderer
  diagnostics below it. Whether it shows saved or live data, and on which connection by name.
- **Open in Crystal Reports**: opens the saved file with Windows' default program for `.rpt`.
  If none is registered, it says so.
- Nothing on the screen edits the report.

## Testing

- Rust: module gating; the tools against the six sample reports in `Out\Sample Report`
  (copied into the test fixtures with the bundle): open, read, edit-and-save (reopened file
  matches the edit and nothing else, using the bundle's diff), refused batch reloads, new,
  check, compare, saved-data preview. The guard's new allowance, and every refusal listed
  above, on both connection kinds. No test needs a database or the designer.
- Front end: the screen's states (no report, a preview, diagnostics, saved vs live note,
  module off), and the Open in Crystal Reports button.
- A live-data preview and opening in the designer need a real Dev database and Crystal
  Reports: the owner checks those by hand.

## Left to the builder

- Where the vendored copy sits in the repo and how its workspace tables are carried, within
  the Constraints.
- Tool names and exact JSON shapes, matching TCM's existing MCP style.
- How the PDF reaches the screen and is displayed (WebView2 can show a PDF).
- Splitting the work into tasks and the order they ship in.

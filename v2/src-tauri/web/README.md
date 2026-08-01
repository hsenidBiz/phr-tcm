# `src-tauri/web`

The CSS, HTML and JS for the pages this app opens in a **real browser** —
the execution report and the test-case view/export. Not the app's own UI:
that is React, under `v2/src`, and none of it comes through here.

These used to live as `r#"..."#` string constants inside the Rust files,
which meant a third of `import_parser/html.rs` was stylesheet with no
syntax highlighting, no formatter and no linter. Each one is now a real
file of its own type, pulled back in with `include_str!` — so the bytes are
still baked into the binary at compile time and nothing is loaded at
runtime, but the Rust files are left holding only logic.

| File | Pulled in by | What it styles |
| --- | --- | --- |
| `scheme-switch.css` | `src/webtheme.rs` | The light/dark switch in the corner |
| `scheme-switch.html` | `src/webtheme.rs` | That switch's markup |
| `scheme-switch.js` | `src/webtheme.rs` | Flips `data-scheme`, remembers the choice |
| `cases-page.css` | `src/import_parser/html.rs` | The test-case view/export page |
| `cases-page.js` | `src/import_parser/html.rs` | Its collapsing, filtering and notes toggle |
| `cases-notes.js` | `src/import_parser/html.rs` | Autosaving comment boxes → the loopback listener |
| `report-page.css` | `src/report.rs` | The execution report |

## Editing these

- **Colours come from CSS variables, never literals.** `webtheme.rs` emits
  a `:root` block per scheme from the palette the running app sends, so the
  page arrives wearing the app's theme and can be flipped once open. A
  hardcoded colour is invisible on at least one of the five themes — most
  reliably on OLED black.
- **No external requests.** These pages are written to a temp file and
  opened over `file://`, and are often saved and mailed on. A CDN font or a
  remote image is a broken page on someone else's machine.
- **Keep them free of comments that name internals.** Everything here is
  inlined verbatim into a page a user can View Source on.
- The Rust doc comment directly above each `include_str!` explains *why*
  that file is the way it is; this table only says where each one goes.

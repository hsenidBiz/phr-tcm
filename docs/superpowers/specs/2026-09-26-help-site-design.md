# "How To Use" help site - design

Date: 2026-09-26. Owner-approved in conversation: bundled in the app,
screenshots automated on demo data, the design below.

## Goal

A **How To Use** button in Settings opens a documentation website in the
person's default browser. The site is sectioned per screen and explains
every button and option in plain task language, with real screenshots of
the app. It must look modern and polished enough to impress on first open.
It is for the QA people who use the app day to day.

## What it covers

The app shell - sign-in, the context bar (organisation, project, PBI
picker), the sidebar, the notification bell, the command palette - and
every generally available screen:

- Test cases: Manual Entry, Import File (queue, review, upload, share
  links, recent imports, specs and comments, View in Browser, Test map),
  Update Test Cases, View Test Cases, Run Tests (and the runner window),
  Search Suites, Suite Management, AI Tools.
- Work Manager: Pull Requests, Board (swimlanes, the work item drawer), New
  Work Item.
- Settings (every section a normal install shows).
- The browser pages the app opens (review page, test map).

## What it never shows or names

Auto Run; hidden features (the list lives in docs-site/src/guard.test.ts);
the dev panel and demo-data controls; anything else gated to dev builds or
those hidden features. Not in text, headings, screenshots, alt text,
search index or file names. A test enforces the text side (below).

## Global constraints

- The site ships inside `v2.exe`: releases pack only the exe
  (scripts/pack.ps1), so nothing may depend on files beside it.
- Works offline, from disk (`file://`): no CDN, no web fonts from the
  network, no module scripts (Chromium blocks `type="module"` on
  `file://`); one HTML file with its script and styles inlined, images in
  a sibling folder.
- No real Azure DevOps data in any screenshot: captures run on demo data.
- User-visible copy: plain words, no em dashes, no hidden-feature names.
- App conventions still hold in the app-side changes: colours from theme
  tokens, icons from src/lib/actionIcons.ts, Rust tests only in
  src-tauri/tests/suite/, bindings generated, errors name no URL.
- One build/test command at a time (shared machine).

---

## 1. Architecture

```
docs-site/                      source of the site (not shipped as-is)
  index.html, src/*.ts, src/styles.css
  content/<screen>.ts           typed content per screen
  shots/                        captured screenshots + positions (committed)
  vite.config.ts                builds to src-tauri/help/ as one inlined HTML
scripts/docs-shots.mjs          the capture script
src-tauri/help/                 BUILT site (committed): index.html + img/
src-tauri/src/help.rs           embeds src-tauri/help/, writes it out, opens it
```

- **Build**: `npm run docs:build` runs Vite with its own config
  (`docs-site/vite.config.ts`, root `docs-site/`, `base: "./"`), classic
  (IIFE) script output with JS and CSS inlined into `index.html`
  (vite-plugin-singlefile or an equivalent inline step), images copied to
  `src-tauri/help/img/`. The built output is committed, so a release needs
  no extra step and `cargo build` embeds whatever is committed.
- **Embedding**: `help.rs` embeds `src-tauri/help/` with the `include_dir`
  crate (new dependency, one small crate). On open it writes the files to
  `<app local data>/help/<app version>/` if that folder is missing or
  incomplete, then opens `index.html` with `tauri_plugin_opener::open_path`
  (the same call the report pages use). Old version folders are removed
  when a new one is written.
- **Command**: `open_help() -> Result<(), String>`; failures log the raw
  error and return `Could not open the help pages. Settings, Logs has the
  details.`
- **Settings**: a new "How To Use" section directly above "Interface tour":
  one line ("A guide to every screen and button, in your browser.") and a
  **How To Use** button (an icon from actionIcons, e.g. the existing
  help/book icon or a new `IconHelp` named for what it does). Clicking
  calls `open_help`; an error shows as a toast.

## 2. Screenshots, automated

- **Capture mode** (dev builds only, like the dev panel): a localStorage
  flag `tcm-v2-dev-capture` = "on" that, on reload, hides the dev panel,
  every dev-only or hidden-feature-gated sidebar entry (Auto Run) and anything else
  a normal install does not show, and turns off the interface tour and
  update prompts so they cannot cover a shot. It is gated on
  `import.meta.env.DEV`, so release builds never contain it.
- **Script** `npm run docs:shots` (scripts/docs-shots.mjs, playwright-core,
  already a dependency via e2e-smoke):
  1. Requires the dev app running with WebView2's CDP port open: the
     script tells the person to start it with
     `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`
     (a `npm run docs:dev` script sets it and runs `tauri dev`), connects
     with `chromium.connectOverCDP`, and fails with that instruction if the
     port is closed.
  2. Sets demo data on + capture mode on + a fixed viewport (1440x900) and
     reloads.
  3. For each theme in [light, slate]: walks a scripted route of screens
     and states (a selection made, a drawer open, a modal open, the
     notification panel open, the runner window) and saves a JPEG per
     state to `docs-site/shots/<theme>/<shot>.jpg`.
  4. For each documented control in that shot, finds it by role and
     accessible name (the same names the content uses) and records its
     bounding box to `docs-site/shots/positions.json`
     (`{ [shot]: { [controlId]: {x,y,w,h} } }`, in shot pixels). A control
     that cannot be found fails the run and names it - the docs cannot go
     stale silently.
- The route and the control list come from the content files (below), so
  there is one source of truth for "what is documented".

## 3. The site ("wow")

- **Look**: the app's own palette (slate/green dark, light variant), Fira
  Sans bundled locally, generous spacing, glass panels, subtle gradients
  and glow in the accent, crisp type scale. Follows the system theme with
  a toggle; every screenshot swaps to the matching theme.
- **Hero**: the app in a floating window frame with a soft accent glow,
  a one-line promise, and a **Quick start** strip: sign in -> pick a PBI
  -> import or write cases -> review -> upload -> run, each step linking
  to its section.
- **Navigation**: a sticky sidebar grouped (Getting started, Test cases,
  Running tests, Work Manager, AI tools, Settings), scroll-spy highlight,
  smooth scroll, deep links (`#run-tests/run-in-runner`).
- **Interactive annotated screenshots** (the centrepiece): each screen
  section shows its screenshot with numbered markers on every documented
  control (from positions.json). Hovering a marker or its row in the
  control list beside it spotlights that control (the rest of the shot
  dims, the control outline glows) and shows its caption. Keyboard: the
  list rows are focusable and do the same. On narrow windows the list sits
  under the shot and markers stay.
- **Search**: Ctrl+K (and a search box) over every screen, control and
  tip, with fuzzy matching; Enter jumps to and flashes the control.
- **Details**: tip and gotcha callouts, keyboard shortcuts as key caps,
  "what you'll see" state notes (empty, loading, errors), a "Common tasks"
  page of short recipes linking into the screen sections.
- **Motion**: section reveals and marker pulses; all off under
  prefers-reduced-motion.
- **Accessibility**: real headings, alt text for every shot, focus
  visible, contrast meeting WCAG AA in both themes.

## 4. Content

- One typed file per screen in `docs-site/content/`:

```ts
export const screen: Screen = {
  id: "run-tests",               // anchor
  title: "Run Tests",
  group: "Running tests",
  summary: "...",                // what the screen is for, 1-3 sentences
  shots: [{ id: "run-tests-selected", route: [...capture steps], alt: "..." }],
  controls: [                    // in on-screen order
    { id: "run-in-runner", shot: "run-tests-selected",
      locate: { role: "button", name: /Run \d+ in runner/ },
      name: "Run N in runner", does: "...", tips?: ["..."] },
  ],
  tips: ["..."],
};
```

- Text is written from the source of each screen (not guessed), in plain
  task language, and covers every button, toggle, field and menu item a
  normal install shows on that screen, in on-screen order.
- A "Getting started" page (sign-in, choosing organisation, project and
  PBI, the sidebar, the bell, the command palette) and a "Common tasks"
  page.

## 5. Testing

- Vitest:
  - every content control has a `locate` and belongs to a shot that
    exists; every shot id is unique; every image the site references
    exists in `src-tauri/help/img/` for both themes;
  - positions.json has an entry for every control;
  - the built `src-tauri/help/index.html` (and the content sources)
    contain none of the hidden-feature terms (the list lives in
    docs-site/src/guard.test.ts): "Auto Run", "autorun",
    "dev panel", "demo data" (case-insensitive, word-bounded where
    needed); image file names likewise;
  - the built HTML has no `type="module"` script and no `http://` /
    `https://` asset reference (offline, `file://`-safe);
  - the Settings section renders a **How To Use** button that calls
    `openHelp`, and an error shows as a toast.
- Rust (tests/suite): `help` writes every embedded file to a temp folder,
  skips the write when the version folder is complete, rewrites an
  incomplete one, removes older version folders, and names no URL in its
  error.
- Capture mode: a vitest that with the flag on in a DEV render the dev
  panel and the Auto Run nav entry are absent.
- Owner hand check: open it from the installed build; the look.

## Out of scope

Hosting online; localisation; a video tour; documenting hidden features;
auto-regenerating shots in the release script (shots are regenerated by
hand with `npm run docs:shots` when the UI changes).

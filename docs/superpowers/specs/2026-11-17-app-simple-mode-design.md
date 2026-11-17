# App Simple mode - design

Agreed with the owner on 17 November 2026.

## Intent

The plan is to roll out Auto Run first. A proper workflow for creating test cases comes later. Until then, the app should show only what that Auto Run-first way of working needs, so people aren't faced with screens they shouldn't touch yet.

This is a separate switch from Auto Run's own Simple and Advanced modes (`docs/superpowers/specs/2026-11-17-autorun-simple-mode-design.md`). The two are independent.

## Goals

- A **Simple mode** toggle in Settings.
- When it is on:
  - the sidebar and the Ctrl+K command list no longer show **Import Test Cases**, **Update Test Cases**, **Search Suites**, **Suite Management** or **API Templates**;
  - the **Modules** page in Settings is hidden.
- Every module is on in this mode, so there is nothing to choose:
  - Database Access, Writing Style, Auto Run, API Templates and Playwright.
  - The held-back report module stays as it is: off unless the machine has unlocked it.
- API Templates keeps working in the background with all its features. The assistant keeps every API Templates tool, and the write switch is on (see section 3).
- **Defaults:**
  - **new installs** start with Simple mode on;
  - **existing installs** stay as they are, with Simple mode off ("for now").

## Non-goals

- No change to what any hidden screen does. Their commands and the assistant's tools keep working; only the person's way in is hidden.
- No per-screen show or hide list (approach B was not chosen).
- No change to Auto Run's own Simple and Advanced modes, or to which mode it defaults to.

## Anti-goals

- Turning Simple mode off and finding modules switched off, or settings lost. Turning it off only brings the screens back.
- A hidden screen still reachable through a stray link, notice or shortcut that leaves the person on a blank or half-working page.
- Turning back on a write switch that someone deliberately switched off.

## Approaches considered

- **A. One app setting that the navigation reads** (chosen by the owner).
- B. A general show/hide list per screen, with Simple mode as a preset. Rejected: it adds a screen-by-screen list nobody asked for, and it reopens the Modules question.
- C. Hide the screens through the module switches. Rejected: Import, Update and the Suites screens are not modules.

## Design

### 1. The setting
- **Storage:** `app_simple_mode: Option<bool>` in Rust app settings, next to `autorun_mode`. `None` means never decided.
- **Default:** decided once, at start-up, with `modules::existing_install(dir)` taken BEFORE anything writes the data folder:
  - a new install gets `Some(true)`;
  - an existing install gets `Some(false)`.
  
  The toggle in Settings writes the value. Once a person picks, the start-up decision never changes it.
- **The toggle:** in Settings, on the main settings page, as a switch titled "Simple mode", with one plain line saying what it hides. It is not offered while an unattended Auto Run run is going, because turning it on switches modules; it shows disabled with a title saying why.

### 2. What is hidden
- **Sidebar:** the rows `import`, `edit`, `suites`, `manage` and `apitemplates` (`src/components/Sidebar.tsx`).
- **Ctrl+K command list:** the entries that open those screens.
- **Settings:** the "Modules" card that opens the Modules page.
- **Way in:** any other way into a hidden screen opens the **Auto Run** screen instead. That covers a notice or bell entry, a "go to" link such as Auto Run's "open the Fixtures tab", a remembered last screen, and the developer panel. One function decides "is this screen hidden", and every navigation entry point uses it.
- **Shown as today:** Manual Entry, View Test Cases, Run Tests, AI Bridge, Auto Run, Playwright, the Work Manager and Settings.
- **Turning Simple mode off** shows everything again at once. The Modules page comes back with every module still on.

### 3. Turning Simple mode on
Turning it on, including the start-up default for a new install, does these things in one action:
- **Modules:** switches every module on in tree order (parents first): Database Access, Writing Style, Auto Run, API Templates, Playwright. The held-back module is left alone.
  - Switching Auto Run on this way goes through the same path as the Modules page. So the Auto Run Simple-mode spec's recommended features apply too, under that spec's own "never on before" rule.
- **API Templates write switch:** turned on for every environment where a person never set it. This is the same rule the Network tools use:
  - a person's choice is recorded with a marker;
  - an environment someone switched off stays off.
  
  The owner chose "all features enabled and available to the assistant". So this includes real, non-test environments that nobody has set.
- **Network tools:** these come on through the Auto Run recommended features. Nothing else is added here.
- **When a switch-on fails** (for example, a module refused because something is busy), Simple mode is still saved. Settings then shows one line naming what could not be switched on, and the person can retry with the toggle.

### 4. Assistants
- No assistant tool can change Simple mode.
- The assistant's guides do not tell a person in Simple mode to open a hidden screen.
- Where a guide or tool answer would send the person to API Templates, Import, Update or the Suites screens, add a line: if the app is in Simple mode, the assistant does that part itself or asks the person to turn Simple mode off in Settings.
- The bridge context carries `app_simple_mode`, the same way it carries the Auto Run mode. `get_current_pbi` returns it when known.

### 5. Guides
- **How To Use:**
  - a short "Simple mode" section in Settings;
  - the Getting started text names it.
- **Hidden screens in the guide:** the guide still documents them, because someone may turn Simple mode off. Each such section gets one line: "Hidden while Simple mode is on (Settings)."
- **Changelog:** one end-user line.

## Constraints
- **All `CLAUDE.md` rules apply:**
  - theme tokens only, and icons from `actionIcons.ts`;
  - generated bindings;
  - the ui-consistency gate, never weakened;
  - Rust tests go in `tests/suite/`;
  - no URL or path in user text;
  - hidden features are never named.
- **The development build** (`dev_identity`) gets its own default like any install. A dev build may start with every module on today (`modules::initial`); that is fine.
- **Off means unchanged:** with Simple mode off, the app is exactly today's.

## Left to the builder
- Where the "is this screen hidden" function lives, and how each navigation entry point calls it.
- Where the toggle sits on the Settings page, and its explaining line, matching the page's existing switches.
- How the start-up default is taken before the data folder is written. This is the same lesson as the Auto Run work's `auto_run_ever_on`.

## Tests (outline)
- **Default:**
  - a new install starts on and switches the modules on;
  - an existing install starts off and nothing changes;
  - a picked value is never overwritten.
- **Hidden screens:** each of the five sidebar rows, the matching command-list entries and the Modules card are hidden when on, and shown when off.
- **Way in:** a notice, a link or a remembered screen pointing at a hidden screen opens Auto Run.
- **Turning on:**
  - every module comes on in order;
  - the held-back module is untouched;
  - the API write switch comes on only where never set;
  - a person-set off stays off;
  - a failed module switch leaves Simple mode saved and shows the line.
- **Turning off:** the screens come back and the modules stay on.
- **Assistants:** the bridge context and `get_current_pbi` carry the mode, and no tool sets it.

## Corrections and calls after the builder check (17 November 2026)

**Where things live.**
- Modules, the Auto Run mode and the video setting live in Rust. The Network tools and the API write switch are per-environment webview storage.
- So "turning it on" is two halves. Rust switches the modules. The webview then finishes the per-environment switches: right after the toggle, or on the first load after a new install's start-up default. The "could not switch on" line covers both halves.
- Switching a module on is never refused for being busy; only a failed save can stop it. The toggle is still disabled during an unattended run, but only so nothing changes under a running run.

**Simple mode switching API Templates on is not a person's choice (coordinator's call).** Simple mode's switch-on must not set `api_templates_set`. Only the Modules page, a person, does. Simple mode's switch-on reuses the module tree and `settle_switch`, with a flag saying it is not a person's choice.

**API write switch markers (coordinator's call).**
- `saveApiWrites` gains a person-set marker, as `networkTools.ts` has.
- **Existing environments:** the first start of this version writes the "set" marker for both switches (API writes and Network tools) on every environment that exists then. Their current state is treated as a person's choice, so nothing an existing user turned off is ever turned back on.
- **Environments created from then on** have no marker until a person sets them.

**Environments created later while Simple mode is on (coordinator's call, from the owner's "all features enabled and available to the assistant").** A new environment starts with the API write switch and the Network tools on. A person can switch either off, and that choice is kept. With Simple mode off, new environments start off, as today.

**The held-back module (coordinator's call).** It stays off in Simple mode. To switch it on, a person turns Simple mode off and uses the Modules page. The goal line is reworded: "The held-back module is never switched on by Simple mode."

**Ctrl+number shortcuts (coordinator's call).** They follow the visible sidebar order, as today, so hiding five rows renumbers them. The guide's shortcut text says the numbers follow the sidebar order, and gives the Simple-mode numbers next to the full ones.

**More ways in to cover:**
- Auto Run's own "open Modules" link (`onOpenModules`, `startOnModules`) is hidden or inert in Simple mode.
- PBI notifications open Import today (`openPbiById(..., "import")`). In Simple mode they open Auto Run on that PBI.
- The existing redirect off a hidden screen (`useModuleGuards`, `src/app/useShellEffects.ts`) sends the person to Manual Entry. Extend it: a screen hidden by Simple mode redirects to Auto Run, using one "is this screen hidden" function beside `sectionShown` and `shouldLeaveHidden`.

**First-run tour (owner's answer).** In Simple mode, the guided tour (`src/tour/tourScript.ts`, started by `useAppTour`) leaves out the stops for hidden screens: Import (both stops), Update, Search Suites and Suite Management. It covers only what is visible. The tour's tests cover the Simple-mode stop list, and confirm the full list is unchanged with Simple mode off.

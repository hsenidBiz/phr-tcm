# Guided Tour — design

**Status:** approved brief (user, 2026-09-02), to be implemented by
`docs/superpowers/plans/2026-09-02-guided-tour.md`.

## Problem

The first-run walkthrough (`src/components/UiTour.tsx`) is a ring-and-caption
tour: it spotlights the sidebar icons and the context bar where they already
sit, reads out a paragraph per icon, and never moves. A new user finishes it
knowing what the icons are called, not what the app does. Every screen it
describes is empty behind the dimmer, because a new user has no project, no
backlog item and no cases yet.

## What we are building

A guided tour that **drives the app**: it switches tabs, crosses into the Work
Manager, and shows each area **populated with sample data**, explaining at most
two things per tab in one or two plain sentences.

### Goals (verbatim from the brief)

1. The user comes away with a basic idea of how the app works and what each
   area does.
2. Short and sweet — do not overwhelm.
3. While the tour runs the app is fully restricted and non-interactable; the
   **Skip tour** button ends it and returns the app to its normal state, with
   no sample data left behind.
4. No technical language, and nothing about the app's internals — only the
   front-facing UI and the tools on offer.
5. The **AI Bridge** may go deeper, and may present the fuller screen a new
   user would not otherwise see (a new user only ever gets the "Working
   repositories" card, because the rest of that tab is gated behind choosing a
   repository).

### Explicit constraints

- **At most two spotlights per tab.** AI Bridge is the one exception (four,
  per goal 5).
- The tour visits **both areas** — Test Case Manager tabs and Work Manager.
- Sample data is shown for the duration of the tour only. Nothing it displays
  may be written to disk, sent to Azure DevOps, or survive the tour.
- The tour must leave the user exactly where they were: same tab, same
  organisation / project / backlog item, same Work Manager state.

## Approach

**Sample data.** A small fixture set (`src/tour/tourData.ts`) served by
temporarily replacing the app's data calls (`src/tour/tourBackend.ts`) while
the tour is open, and restoring the originals when it ends. This is the same
mechanism the dev-only demo mode uses, with two differences: it is reversible
(the originals are saved and put back), and it is small enough to ship.

**Nothing persists.** Six writers could otherwise leave sample data behind,
and each is switched off for the duration:

| Writer | How it is stopped |
|---|---|
| The on-disk read cache (`lib/localCache.ts`) | suspended — reads return nothing, writes are dropped |
| Saved context (`lib/prefs.ts`, written by App) | the save is skipped while the tour runs; the real context is restored when it ends |
| React Query's in-memory cache | the toured screens run against a throw-away cache that is discarded at the end |
| Field choices per project (`lib/fieldPrefs.ts`) | the save is a no-op while the tour runs |
| The test-suite seed (`lib/suiteSeed.ts`) | one guarded writer, covering both the screen's write and App's |
| The draft queue (`hooks/useQueue.ts`) | the tour serves a fixture and the hook touches storage in neither direction — no read, no write, no removal |

The last three were found during implementation, not design, and they share a
shape worth stating: **one guard at the single chokepoint that writes a key,
reading the tour's own running flag.** A per-caller toggle has to be remembered
at every call site and on every exit path; a leaf guard cannot fall out of step
with them. Two of the three had a *second* writer that the first pass missed —
which is why the rule is one guard per key, not one guard per screen.

The draft queue is the case that does not fit the pattern, and it says
something about the sample-data mechanism itself: swapping the app's data calls
only covers regions fed by those calls. Anything a screen reads from local
storage instead — the queue is the only such region on the route — gets no
sample data and needs its own answer. When judging coverage, the question is
"what feeds each area the tour points at", not "what calls do these screens
make".

**AI Bridge.** The tab hides everything but the repository card until a working
repository is chosen. For the tour an in-memory override supplies a sample
repository so the full tab is visible; clearing the override restores the real
answer. Nothing is written to the saved repository list.

**Restriction.** The app behind the tour is made inert (no clicks, no focus,
no keyboard shortcuts, no command palette). Only the tour card responds:
**Back**, **Next**, and **Skip tour**. Escape does nothing — leaving is a
deliberate click.

## The route

18 stops, one or two short sentences each, roughly two minutes.

| # | Where | Spotlight |
|---|---|---|
| 1 | — | Welcome (centred card) |
| 2 | Context bar | Organisation / project |
| 3 | Context bar | Backlog item picker |
| 4 | Manual Entry | New test case form |
| 5 | Manual Entry | The batch waiting to be sent |
| 6 | Import File | Bring cases in from a file |
| 7 | Update Test Cases | The list of existing cases |
| 8 | View Test Cases | Read-only view |
| 9 | Run Tests | Cases with their last result |
| 10 | Test Suites | The folder tree |
| 11 | AI Bridge | Working repositories |
| 12 | AI Bridge | Connect your AI tools |
| 13 | AI Bridge | What an assistant may do |
| 14 | AI Bridge | Company database |
| 15 | Work Manager · Board | The columns |
| 16 | Work Manager | Pull Requests |
| 17 | Context bar | Settings (incl. replaying the tour) |
| 18 | — | Closing card (centred) |

## Out of scope

- Auto Run (development-only tab).
- Any change to what the tour is triggered by: first sign-in, and the
  **Show UI tour** button in Settings.
- Interactive practice ("now you try") — the app stays locked throughout.

---

# Two import fixes — design

**Status:** approved brief (user, 2026-09-02). Shipped in the same release as
the tour; implemented as Tasks 8 and 9 of the plan.

## 1. The queue's main button on a long list

**Problem.** Importing 100+ cases makes the queue longer than the window, and
the only way to send them is to scroll to the very bottom, where **Review N
test cases** (and then **Confirm & create N**) live.

**Wanted.** That button becomes sticky, the way the **Collapse all** button
already is. When the user scrolls far enough that the real button is on
screen, the sticky one goes away — with an animation — and the real control
takes over. The sticky button looks exactly like the button does today.

**Approach.** A portalled copy of the same button, shown while an
`IntersectionObserver` reports the real action row off screen, fading and
sliding out when it comes back into view. Portalled for the same reason the
Collapse all pill is: `AnimatedContent`'s transform would otherwise make
`fixed` mean the scrolling region instead of the window.

**Decisions.**
- **Bottom centre.** Bottom-left is the Collapse all pill; bottom-right is
  where toasts appear.
- **`aria-hidden`, not focusable.** It duplicates a control already in the
  page. A screen reader reaches the real one by navigation, not by scrolling,
  and two controls with the same name is a worse outcome than none.
- **Not over the confirmation gates.** The armed "Yes — create N" panel and the
  duplicate-title gate exist to be read before a write that cannot be undone;
  the floating copy hides while either is up.

## 2. Watched-file changes should pile up

**Problem.** When an assistant edits a watched `.json` file, the change panel
reports what moved. A second save **replaces** that panel, so the first edit's
changes disappear — the only place they can be seen again is the review stage.

**Wanted.** The user decides when changes are cleared: the panel keeps
accumulating, and the **X** beside it is what empties it. After that, the next
save starts a fresh report.

**Approach.** The panel already survives until dismissed; what it loses is the
*earlier* changes. So each watched file gets a pile with two baselines —
the queue and the file's contents as they were when the pile started (a first
save after a dismissal, or after a fresh import). Every later save recomputes
the panel's contents as one diff from those baselines, using the existing
`syncFromFile`, which measures changes queue-vs-file. The queue keeps syncing
against the previous snapshot exactly as it does now; only what is *displayed*
changes.

**Decisions.**
- **One pile per watched file**, each with its own X — several files can be
  watched at once, and merging their edits into one panel would lose which
  file did what.
- **A file edited back to its starting state clears its own pile** — there is
  genuinely nothing to report.
- **Warnings show the latest parse's count**, not a running total: they
  describe the file as it stands, not its history.
- Accepted consequence: if the user edits the queue in-app between two saves,
  the accumulated report is measured against the queue as it was when the pile
  started. That is the honest answer to "what has this file done since I last
  looked".

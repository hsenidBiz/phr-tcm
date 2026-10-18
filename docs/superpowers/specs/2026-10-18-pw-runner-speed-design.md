# Playwright Runner: open in about a second, however many tests

Date: 2026-10-18. Status: design, from the conversation with Avin ("do all three").

## Why

The Runner tab is slow to open, and it gets slower as the suite grows. On Avin's clone
(496 spec files, about 2,950 listed tests, 417 past reports) the app's own timings show
`pw_list` taking 10 to 97 seconds and `pw_reports` up to 33 seconds on every open after a
restart. Three causes, measured:

1. **The listing is redone after every restart.** Building the tree runs
   `npx playwright test --list`, which compiles every spec (15.5 s from the command line,
   44-97 s inside the app). TCM keeps the listing, but a listing of 1 MB or more is kept in
   memory only (`PW_LIST_PERSIST_MAX`), and Avin's is 1.27 MB, so the log says "kept for this
   session only" every time. The kept listing also expires after 8 hours (`PW_LIST_TTL`) even
   when nothing changed.
2. **Past reports are re-read after every restart.** The reports list needs five counts per
   report, but their cache is session-only, so every restart parses all 417 report files (up to
   1.5 MB each).
3. **Any spec change means a full wait with an empty screen**, and when the tree arrives every
   branch is unfolded, so about 1,500 tests and their tick boxes are drawn at once and redrawn
   on every tick.

## Goals

- Opening the Runner after a restart shows the tree in about a second when the clone has not
  changed, at any number of tests.
- When the clone has changed, the last tree shows at once while the new listing runs; nothing
  can be run against the old tree.
- The tree stays quick to draw and tick at several thousand tests.

## Non-goals

- Making Playwright's own listing faster, or listing without Playwright.
- Changing what the tree shows, how tests are picked, or how a run works.
- Caching in the Playwright repo, or reading the repo's own `.qa-cli/inventory.json`.

## Anti-goals

- A run, a preview or a dry count working from a tree that is not the clone's current one.
- A second caching mechanism: everything goes through the existing Rust cache
  (`src-tauri/src/cache/`) and the webview cache (`src/lib/cache.ts`), per CLAUDE.md.
- Cached data surviving a sign-in by a different account (both caches already wipe then; the
  new pieces must too).

## Decided

1. **Keep the listing on disk at any size.**
   - The Rust cache gains a way to keep one large value in its own file beside `cache.json`
     (which is rewritten whole on every change, the reason for today's 1 MB limit). It is part
     of the same cache: the same keys file, the same wipe on `clear` and on a different
     account's sign-in, and putting a new Playwright listing still drops the previous clone's.
   - The kept listing holds only what the tree reads (on Avin's clone 1.36 MB becomes about
     0.74 MB).
   - The listing no longer expires after 8 hours: the clone's fingerprint already decides when
     it is out of date. A kept listing older than 30 days is dropped.
   - Two windows, or a window and a run, asking for the listing at the same time produce one
     listing, not two.
2. **Show the last tree at once, refresh behind it.**
   - Opening the Runner first asks for the kept tree, which never runs Playwright. If it is the
     clone's current one, that is the tree.
   - If the clone changed since (or there is no kept tree), a new listing starts straight away.
     The old tree, if any, stays on screen with a line saying it is being updated, and is
     replaced when the listing finishes. With no old tree the screen says "Listing the tests…"
     as today.
   - While the tree on screen is not the current one, Run, Preview and Debug are disabled, with
     a sentence saying why. Runs and previews in Rust keep using only a current listing, as
     today.
   - Past reports keep their five counts in the durable cache, keyed by the report's file size
     and modified time, so a report is parsed once ever, not once per restart.
   - The tree itself (the listing joined with ADO ids and last results) is built once per
     clone state and kept for the session.
3. **A lighter tree on screen.**
   - The tree opens folded at the feature level: folders down to each feature are open, each
     feature's specs and tests are folded. What the person unfolds or folds is remembered per
     clone.
   - Only the rows on screen are drawn: the visible branches are flattened into one list and
     drawn through a virtual list.
   - How many tests each branch has ticked is worked out once per change of ticks, not per
     branch on every draw.
   - Every name a screen reader or a test uses today ("Tick …", "Fold …", "Unfold …", the Tests
     list) stays the same.

## My calls (Claude), for review

- The large-value file is per key, named from a hash of the key, in a `cache/` folder beside
  `cache.json`. Only the Playwright listing uses it for now.
- 30 days for the kept listing: a month-old listing is more likely to mislead than help, and
  the fingerprint is the real guard.
- The virtual list uses `@tanstack/react-virtual` (same family as the react-query the app
  already uses), the one new dependency.
- "Folded at the feature level" means: on first open, folders are unfolded down to the
  feature folder, and the feature folders are folded. A person's own folds replace this per
  clone.

## Testing

- Rust: a listing over 1 MB survives a restart of the cache; the large file goes on `clear`
  and on another account's sign-in; a new clone's listing removes the old file; a kept tree is
  marked stale when the fingerprint differs; two concurrent listing requests run one listing;
  report counts are read from the durable cache when size and modified time match.
- Webview: a stale tree shows with the updating line and disabled run buttons, then is
  replaced; the default fold and remembered folds; a long tree draws a bounded number of rows;
  tick counts.
- By hand, on Avin's clone: open the Runner after a restart (aim: tree in about a second, the
  log's `pw_list` timing confirms); change a spec and reopen (old tree, updating line, then the
  new tree).

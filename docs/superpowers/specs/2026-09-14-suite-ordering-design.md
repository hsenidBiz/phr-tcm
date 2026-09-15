# Suite ordering: blocks, groups and files — design

Agreed with the user on 2026-09-14. This is the record the implementation plan argues from.

## The problem

Suite Management can reorder a suite's cases, but only one row at a time, and "Apply tester order from file" takes one file and sorts its cases by their `tester_order` numbers. Two things went wrong in practice: a set of cases laid out deliberately in a file landed in the suite in a different order (the numbers, not the rows, decided), and arranging a 300-case suite one row at a time is not workable. The user also wants to think in title groups ("Alerts", "Filter Card", …), which the app already recognises elsewhere.

## Decisions

1. **Every ordering rule is a pure function in `src/lib/suiteOrder.ts`**, tested on its own. The list component draws and dispatches; it holds no ordering logic. (Chosen over bolting behaviour onto the component, or rebuilding the list as a tree.)
2. **A ticked selection drags as one block.** Tick rows, drag any ticked row: the whole selection moves together, keeping its relative order. Dragging an unticked row moves only that row, as today. Move up / Move down on a ticked row moves the block one step, so keyboard users get the same. It is the one selection the screen has: what is ticked is also what Copy to suite and New suite act on.
3. **Group by title is a switch on the suite**, remembered app-wide under `tcm-v2-group-manage` like the other screens' switches. Turning it ON arranges the list so each group is contiguous — groups in the order they first appear, cases keeping their order inside — and shows a header per group. If that changed the order, the list is dirty and Apply order lights up, exactly as after a drag. Turning it OFF hides the headers and leaves the order alone. A suite opened with the switch already on is NOT rearranged (nothing the user did not ask for may make the list dirty): its headers reflect the order as it is, so a split group shows as two sections until the user consolidates it.
4. **Headers are sections, and sections are runs**: consecutive cases with the same group, in the current order. Each header has a tick (selects the section's cases) and moves as a block by drag or by its own Move up / Move down. Rows can still be dragged individually inside or across sections; the headers redraw from whatever order results.
5. **"A–Z groups"** is a button beside the switch, shown only while grouping is on: it makes every group contiguous and sorts the groups alphabetically (case-insensitive), cases keeping their order inside. Ungrouped cases form one trailing "Ungrouped" section in both arrangements.
6. **"Apply order from files"** (renamed: numbers no longer decide) accepts several files. Each file becomes a block **in the file's row order**; `tester_order` is ignored. A dialog lists the chosen files with how many of this suite's cases each one places; the files are dragged (or moved with arrows) into the order their blocks should take. Rules: blocks follow the dialog's order; a case named in two files goes with the first and the dialog says how many such cases there are; cases in no file trail behind in their current order; a file that places nothing here is flagged rather than refused. "Add more files" opens the picker again and appends. Apply arranges the list on screen and closes the dialog; **Apply order still saves** — the file only proposes. The dialog remembers nothing between opens.
7. **The files are never rewritten.** The order lives in the suite; `tester_order` inside the files stays whatever it was.
8. **Saving is unchanged**: one `reorder_suite_cases` call with the final id list. No Rust changes.

## Out of scope

- Ordering across several suites at once.
- Persisting a group arrangement anywhere but the suite itself.
- Any change to what the checkboxes mean for Copy to suite / New suite.

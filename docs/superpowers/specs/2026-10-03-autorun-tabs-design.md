# Auto Run screen in three tabs

Design agreed with the owner on 2026-10-03.

## 1. Why

The Auto Run screen puts everything in one place:
- a header line;
- a Setup card with five rows;
- the test-case toolbar and list;
- the selection bar;
- at wide sizes, a second column of past runs with its own filters.

The owner finds it overwhelming. The goal is a screen that is easier to read and simpler, with **every feature kept**.

## 2. Owner decisions

1. Three tabs: **Test cases**, **Past runs** and **Setup**.
2. The screen opens on **Setup** while something essential is missing, and on **Test cases** otherwise.

## 3. The layout

**Tab bar.** It sits under the screen's header and holds three tabs.
- **Test cases** shows its row count, and **Past runs** shows its run count.
- **Setup** carries a warning icon while something essential is missing: there is no site address, no sign-in, or no accounts. Its accessible name then says so.
- It is a real tab list, with arrow keys between tabs and `aria-selected`, the same pattern as the Templates/Flows tabs on API Templates.

**Which tab opens.**
- The tab is decided once, after the readiness data has loaded: Setup if something essential is missing, otherwise Test cases.
- A tab the person picks is never overridden while the screen stays open.
- Finishing a run's review leaves the person on **Past runs**.

**Test cases tab.**
- **Readiness strip.** One line that replaces today's header line. It shows:
  - the environment name and site host;
  - a tick or a warning for sign-in (saying **Built-in** when the built-in recipe is in effect), the number of accounts, the number of areas, and test files;
  - **Open setup**, which switches to the Setup tab.
- Test files warn only when a saved script's `upload` action names a file that is not in the Test files folder. The warning says how many files are missing.
- **Toolbar.** **Group by title** stays visible. A **More** menu holds:
  - **Import scripts**, with the line "One JSON file can carry every case in this PBI." as its description;
  - **Clear scripts**, with its existing confirm step and danger hover.
- **List.** The rows are unchanged: checkbox, id, title, the Suspected defect badge, Script or Add script, and Run.
- **Selection bar.** Unchanged.

**Past runs tab.**
- Today's Past runs panel at full width: the result filters, then the run cards with their counts, Report and review.
- The xl two-column layout goes.

**Setup tab.**
- A read-only line naming the active environment and its database (`<label>: <database> on <server>`), with a pointer to the AI Bridge tab for changing them.
- Today's five Setup rows, unchanged in behaviour: Site address, Sign-in, Accounts, Areas and Test files.
- A one-line note that an assistant's `/tcm:setup` command can walk through this. Auto Run is hidden, so the note never appears outside Auto Run.

## 4. Not changing

These stay as they are:
- the run dialogs, the review, the script editor, and the account, area, test-file, site and recipe dialogs;
- the Auto Run gating, and the rule that it is never named in the changelog or help.

## 5. Testing

- Opening tab: Setup when the site address or accounts are missing, and Test cases when ready. A person's own choice is not overridden.
- The Setup tab's warning icon and its accessible name.
- The readiness strip's ticks and warnings. This includes the missing-test-file count, worked out from scripts' upload actions.
- The More menu holds both actions, and Clear scripts still asks first.
- Past runs renders on its own tab, and finishing a review lands there.
- The Setup tab shows the environment and database line and all five rows.
- Every existing Auto Run test still passes. Where a test depended on the old layout, it is updated deliberately: switch tabs first.
- `src/ui-consistency.test.ts` and `src/a11y.test.tsx` stay unchanged and passing.

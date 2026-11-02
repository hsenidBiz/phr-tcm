# Auto Run Accounts dialog: tabs - short design

Date: 2026-11-02. Owner picked option A.

## Why

The Accounts dialog (`src/screens/AutoRun/AccountsDialog.tsx`) stacks three
jobs in one scrolling column: editing the accounts, Check sign-in (which
lists every saved account a second time), and the assistant's proposed
logins. With a few accounts and proposals it gets long, Save scrolls away,
and Check sign-in (saved accounts only) can disagree with unsaved edits
above it.

## What

- Three tabs under the dialog's title and description: **Accounts**,
  **Check sign-in**, **Proposed** (with its count, e.g. "Proposed 12";
  the tab appears only while there are proposals).
- **Accounts** tab: the editable rows, Add account, Show passwords, and the
  Cancel / Save accounts footer. Unchanged behaviour.
- **Check sign-in** tab: today's section as it is (Walk to, the ticked
  list with last results, Check sign-in / Cancel check). When the Accounts
  tab has unsaved edits, it says so at the top: "Save your changes on the
  Accounts tab first - Check sign-in uses the saved accounts." The check
  stays usable on the saved accounts.
- **Proposed** tab: today's section as it is (search, select all, the list,
  Replace/Keep, Add selected, Dismiss). Adding selected switches to the
  Accounts tab so the person sees them and saves.
- The Cancel / Save footer stays visible on every tab (it saves the
  accounts), so leaving the dialog works the same from any tab.
- The Name column is wider (the four columns become
  `1fr 1.6fr 1fr 1fr`), so names like "Payroll Automation (000002)" show.
- The dialog opens on Accounts.

## Approaches

A (tabs) chosen over B (one list with inline checks and a proposals
banner): A keeps each job whole and is the smaller change; tabs already
exist on two screens. B removed the duplicate list but made rows busier.

## Constraints

- Reuse the tab pattern already used in the app (`role="tablist"` in
  `src/screens/AutoRun/index.tsx` / `ApiTemplates`), its classes and
  keyboard behaviour; colours from tokens only; icons from
  `src/lib/actionIcons.ts`; the ui-consistency gate stays as it is.
- Accessible names of existing controls stay the same (tests and the How
  To Use guide rely on them); the tabs are new named controls.
- How To Use: if a documented shot shows this dialog, retake it (docs
  shots workflow in CLAUDE.md).

## Left to the builder

Exact tab component reuse, where the unsaved-edits note sits, test layout.

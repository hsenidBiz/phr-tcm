# Execution order: one button in Run Tests

Design, agreed with the owner on 2026-09-23. Amends
`2026-09-23-run-order-design.md` §3 (owner rule 4) and §5.1/§5.3.

## 1. Why

Run Tests grew an Order dropdown, a drag handle and Move up/down on every
row, Move group up/down on every group header and a Reset button; Suite
Management grew a separate "Suggested run order" editor with its own Start
from list and Save. The owner found that too many controls at once. All of
it moves behind one **Set execution order** button in Run Tests that opens a
modal. (The runner's "Run next" was removed for the same reason, in 1.25.22.)

## 2. Owner decisions

1. The modal can save an order for **me** (this machine) or for
   **everyone** (the PBI's suggested run order).
2. Reordering happens **only inside the modal**; the Run Tests list only
   shows the chosen order.
3. Suite Management's Suggested run order section is **removed**; its Azure
   DevOps order editor stays exactly as it was.

## 3. The rule, restated (replaces run-order design §3 rule 4)

- The suite's order in Azure DevOps (spec order) changes only in Suite
  Management's Azure DevOps order editor and at upload. Unchanged.
- The shared suggested run order changes only through **Save for everyone**
  in the Run Tests modal, and at upload.
- My order (local) changes only through **Use this order** in the modal.
- The Run Tests list and the runner never change any order on their own.

## 4. Run Tests list

Removed: the `Order` `Select`, row drag (`draggable` rows and their drop
handling), row `Move <name> up/down` buttons, group `Move group <name>
up/down` buttons, and the Reset (to suggested/spec) control.

Kept: grouping, filters, selection, "Run N in runner", refresh. The list is
drawn in the active order exactly as today (`useRunOrder`'s view + reconcile
rules, design §4.4).

Added, where the Order select was:
- A **Set execution order** button (icon from `src/lib/actionIcons.ts`).
- Beside it, muted plain text naming the active order: `Suggested run
  order`, `Spec order` or `My order` (the existing `ORDER_LABELS`).
- The unreadable-file note (`noteFor(reason)`) stays under the toolbar, as
  now.

## 5. The modal

Shared `Modal`. Heading `Execution order`.

- **Start from** (`Select`, aria-label `Start from`), options in this order:
  - `Suggested run order`: present when the file read is `found`; present
    but disabled when it is `unreadable` (the note explains why); absent
    when `none`.
  - `Spec order`: always.
  - `My order`: when this machine has one for the suite.
  - `Tester order from <file>`: one per qualifying watched draft
    (`testerOrderSources(loadWatches(org, pbiId), suiteCaseIds)` from
    `src/lib/testerOrderStart.ts`, unchanged rules: every case has a
    `tester_order`, at least one is in the suite; labels as that helper
    makes them).
  - Initial value: the list's active view (suggested / spec / mine).
- A note under it: `Saved by <saved_by> on <local date>` when the file is
  found; `No suggested run order yet.` when none; `noteFor(reason)` when
  unreadable.
- The list: `CaseOrderList` over the suite's cases (`{id, title}`), in the
  chosen start, reconciled against the suite's cases (design §4.4). Drag and
  Move up/down as in `CaseOrderList`. Changing Start from replaces the list.
- Buttons, left to right: `Cancel`, `Save for everyone`, `Use this order`
  (primary).
  - **Use this order**: if the start is `Suggested run order` or `Spec
    order` and the list is unchanged from that start, save the view only
    (`saveOrderView(key, "suggested" | "spec")`), leaving any My order
    stored. Otherwise save the list as My order (`saveMyOrder`) and the view
    `mine`. Close the modal. When the list starts from My order unchanged,
    it just sets the view to `mine`.
  - **Save for everyone**: opens a confirm step inside the modal with the
    exact sentence `Every tester will see this as the suggested run order
    for this PBI.` and buttons `Cancel` / `Save`. Save calls
    `commands.saveRunOrder(org, project, pbiId, cases)`; groups: the
    started-from file's `area` when the start was a tester-order file, else
    the group the case had in the saved file, else none. On success:
    `toast.success("Suggested run order saved.")`, `setQueryData` + 
    `invalidateQueries` on the shared run-order query (as Suite Management
    did), the view becomes `suggested`, the modal closes. On error:
    `toast.error("Could not save the suggested run order: <message>")`, the
    modal stays open.
  - **Cancel** / closing: nothing changes.
- Only for a requirement suite (a PBI exists, `pbiId > 0`): `Save for
  everyone` is absent otherwise.
- The tour backend stays read-only (no `saveRunOrder` stand-in).

## 6. Suite Management

Remove `SuggestedOrder` (component and tests) and its render in
`SuiteCases.tsx`, and the `Order in Azure DevOps` heading added with it;
the Azure DevOps order editor is otherwise untouched. The shared helpers it
introduced (`runOrderQueryOptions`, `noteFor`, `testerOrderSources`) stay
and are used by the modal.

## 7. Testing

- Run Tests: the list has no Order select, drag, row or group move buttons
  or Reset; the button and the active-order text show; opening the modal
  shows Start from with the right options (found / unreadable disabled /
  none / My order / tester file).
- Use this order: unchanged Suggested or Spec sets the view only (My order
  untouched); a reorder saves My order and switches the list; a tester-order
  start saves My order.
- Save for everyone: confirm sentence; Cancel sends nothing; Save sends
  `save_run_order` with ids in list order and the group rules above; toast;
  the list switches to Suggested; an error keeps the modal open with the
  toast; absent for a suite without a PBI.
- Suite Management: no Suggested run order section; the Azure DevOps editor
  tests still pass.
- `src/ui-consistency.test.ts` and `src/a11y.test.tsx` pass unweakened.

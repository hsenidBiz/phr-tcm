# XiodUI refresh: six controls

Design, agreed with the owner on 2026-09-23.

## 1. What

Six of the app's controls take their look and behaviour from XiodUI
(`xiod-ui`, https://ui.xiod.dev, React 19 + Tailwind 4 on Base UI): the date
picker, checkbox, command palette list, keyboard-key hints, text areas and
toast notifications.

## 2. Owner decisions

1. Use the `xiod-ui` npm package, **pinned to exactly `1.0.3`** (no caret):
   the package is new (repository created 2026-09-12, one maintainer), and
   any script in the app's window can call every app command, so it only
   changes when we choose to update it, after reading what changed.
2. React moves to 19.3 (xiod-ui's peer requirement); `react-dom` with it.
3. Licensing: xiod-ui is PolyForm Perimeter 1.0.1 (source-available; any
   use except a product competing with it). Every release ships its terms
   and its `Required Notice:` copyright line (and those of `xiod-icons`, if
   it is licensed the same way) - see §5.

## 3. How

- Each control is swapped **behind the app's own wrapper** in
  `src/components/ui/` (existing names and props kept), so call sites do
  not change and reverting is one place:
  - Checkbox: `src/components/ui/checkbox.tsx`.
  - Textarea: the `Textarea` export in `src/components/ui/input.tsx`.
  - Date picker: `src/components/ui/datefield.tsx` / `calendar.tsx`
    (today `react-day-picker`).
  - Command: the command palette's list (today `cmdk`); the palette's own
    component keeps its behaviour and shortcuts.
  - KBD: the keyboard hints (wherever the app renders `<kbd>` or a hint
    component) become one shared `Kbd` wrapper.
  - Toast: today `sonner`; call sites keep calling `toast.success(...)`,
    `toast.error(...)`, etc. through one local module that forwards to
    xiod-ui's toast, and the app's single toaster host is replaced.
- Colours come from the app's theme tokens (light and dark), mapped onto
  xiod-ui's styling hooks; `src/ui-consistency.test.ts` stays unweakened
  and the a11y tests pass. Existing behaviour each wrapper's tests pin
  (checked/indeterminate, disabled, keyboard use, focus return, toast
  durations/actions) is kept.
- Packages that nothing uses any more after the swap (`sonner`, `cmdk`,
  `react-day-picker`) are removed.

## 4. Vetting (before anything depends on it)

Read the installed code of `xiod-ui@1.0.3` and its runtime dependencies
(`xiod-icons`, `cn`, `@base-ui/react`, `class-variance-authority`): no
install scripts, no network calls, no `eval`/dynamic code loading, no
storage or IPC access. Record the result.

## 5. Notices

A third-party notices file in the app bundle (and reachable from Settings'
About area if one exists) carrying the PolyForm Perimeter terms URL or text
with `Required Notice: Copyright 2026 ImKKingshuk
(https://github.com/ImKKingshuk)` for xiod-ui (and xiod-icons as its
license requires).

## 6. Testing

Every wrapper's existing tests pass (updated only where markup legitimately
changed, never weakened); new tests for any behaviour the swap adds;
the full frontend suite, typecheck and production build pass.

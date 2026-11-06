# Signed-in check inside a menu — design (short)

Date: 2026-11-06. Status: approved by the owner ("Yes do this change").

## Why

Some sites keep Sign out inside a menu that opens from the signed-in user's
name. When recording a sign-in, the only signed-in check the person can see is
that name, which is wrong for every other account. A save then fails with "the
signed-in check never appeared". Opening the menu during recording does not
help either: that click is recorded as a sign-in step named after the person.

## Goals (owner)

1. While the person is picking the signed-in check, clicks that open menus are
   carried out on the page and are NOT recorded as sign-in steps. The person
   can open their name menu and pick Sign out.
2. Replays pass when that check is on the page, even while its menu is closed,
   because a replay never opens the menu.

## Non-goals

- Recording menu-opening steps as part of the sign-in.
- Changing the check for recordings whose check was picked in plain view.
- Detecting a person's display name as a check (it cannot be known).

## Decisions (mine)

- **Which clicks open a menu, while picking:** the clicked element, or an
  ancestor up to a few levels, carries `aria-haspopup`, `aria-expanded`, or a
  dropdown toggle attribute (`data-toggle`/`data-bs-toggle="dropdown"`). That
  click is carried out and is neither recorded nor picked. Pick mode stays on.
  Any other click picks the check, as today.
- **Fallback for sites with no such markup:** a click with **Shift** held is
  always carried out and never recorded or picked while picking. The picking
  hint says so in one short line.
- **Only for checks picked inside a menu:** when the picked element is not
  visible at rest (hidden once the menu closes, or under an opened menu
  opener), the recipe records that its check may be hidden. Replays then ask
  that it is **present** on the page (`Check::Present`). Every other recipe
  keeps today's **visible** check. A recipe saved before this loads unchanged
  (field defaults to off).
- **Wording (owner, 2026-11-06):** after the person signs in, the dialog asks
  "Now show us how you sign out: open the menu and click Sign out (it won't
  actually sign you out)." in place of today's pick hint.
- The dialog shows "Signed-in check: <words> (inside a menu)" for such a
  check, so the person can see it was understood.
- The existing refusal of a check that names the account stays.

## Constraints

- Rust tests in `src-tauri/tests/suite/` only; regenerate bindings after a
  `specta::Type` change; UI text names no path or URL; theme tokens only.
- Help guide: the Record sign-in section mentions opening a menu to pick
  Sign out, and Shift-click.

## Left to the builder

- How "inside a menu" is decided when the check is picked: record the
  ancestor menu opener seen while picking, or test visibility after the pick.
- The number of ancestor levels to search for a menu opener.

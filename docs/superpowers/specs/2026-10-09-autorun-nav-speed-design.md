# Auto Run: faster navigation and step pictures

Date: 2026-10-09
Status: approved in conversation. A to D are approved, and step pictures are kept but no longer waited for. This file is the written spec.
Evidence: a comparison with PHR-PLAYWRIGHT-AUTOMATION (2026-10-09).
- Our click and wait loop is already close to Playwright's.
- The cost sits around each step: a full home reload before most area trips, a 1.5 s prompt window after each reload, a screenshot awaited after every step, every request paused on Must not save scripts, and the recipe and areas file read from disk on every step.
- Going straight to an area's address is out. PeoplesHR disables direct navigation, and a reload bounces to home.

## A. Open the menu from where the page is

- Today `go_to_module` goes home first, unless a sign-in just left the browser at the path's start. It then reloads the page, waits for the signed-in marker, runs `after_sign_in` again, and then clicks the path.
- New: the trip first tries the path's clicks from the current page, with no reload.
  - **The flip-flop guard.** A path click is skipped when the click after it is already visible and enabled, because the menu is already open there. This stops a sidebar toggle from closing an open menu. The check uses the same visibility rule the click itself uses, with no wait.
  - **Quick give-up.** If any click in the first try fails, the trip goes home the old way (`go_home` / `load_home`) and runs the whole path again. The first try's click limit is `QUICK_TRY_MS = 3000` per click, so a wrong page does not cost the full action timeout.
  - **The arrived check is unchanged.**
- **What stays the same:**
  - The first trip after a sign-in still uses today's rule.
  - `reach_module`'s retry stays as it is: one more full try after a reload.
  - A failure on the old-way try reports exactly as today. A failure that the fallback recovered from is not an error, and it is logged once at INFO.

## B. A shorter prompt window after a trip home

- `after_sign_in`'s prompt window after a `go_home` or `load_home` reload drops from 1500 ms to `HOME_PROMPT_WINDOW_MS = 500`.
- A fresh credential sign-in keeps `FRESH_LOGIN_WINDOW_MS = 5000`.
- A saved-session reuse keeps `PROMPT_WINDOW_MS = 1500`. The late session modal comes 187 to 288 ms after the shell, and only after a fresh login.

## C. Must not save checks only requests that can save

- `Fetch.enable` currently sends one pattern per resource type that can carry a save: `Document`, `XHR`, `Fetch`, `Ping`, `EventSource`, `Other`, plus any type the guard already relies on.
- New: `Image`, `Stylesheet`, `Script`, `Font`, `Media`, `Manifest` and `TextTrack` are no longer paused.
- What the guard blocks is unchanged for every save-shaped request: form posts, XHR and fetch posts, and beacons. It applies to Must not save cases, the sign-in guard and a mapping run alike.

## D. Read the run's files once, unless they changed

- The recipe and the areas file are read once. They are read again only when the file's modified time or size changed since the last read, which is a cheap stat per step.
- This keeps the supervised run's promise that a change between two steps is seen.

## E. Step pictures, kept but not waited for

- Every step still gets its picture, taken of the page after the step. Today the next step waits for the capture and the disk write.
- **New:**
  - The disk write (`store::save_shot`) happens off the step loop.
  - Where the driver allows it, the capture itself is requested and its answer collected later.
  - The next step waits for an outstanding capture only before its own first action that changes the page (click, fill, key, drag, navigate, reload). Reads and checks may run during the capture.
- **Before a case's record is saved,** every outstanding picture is finished, so the record names each picture or none, never a missing file.
- **What stays:**
  - A failed or timed-out capture leaves the step with no picture, as today.
  - The save that `take_save_blocked` reads after the step's last action is still read at the same point. If the capture no longer gives it that moment, a cheap call is made there instead.

## F. Highlight each action, optional

- A **Highlight each action** tick box beside **Watch the browser** in the run options, on by default. It is an app setting (`autorun_highlight`, default true) kept in Rust, so the supervised Auto Run browser reads it too.
- Off: a watched run and the supervised browser skip the outline and the 350 ms pause before each click, fill and drag (`highlight_ms` 0). On: as today.
- An unattended run that is not watched never pauses, as today.

## Errors and limits

- Nothing a run checks changes. The Must not save guard still blocks every save-shaped request.
- No hosts or query strings in logs. No HTTP DELETE. No em or en dashes.

## Testing

- **Rust, with a fake driver:**
  - a trip from inside the app does not reload home;
  - a menu that is already open has its toggle skipped;
  - a failed quick try falls back to home and the full path;
  - after a sign-in the first trip is unchanged;
  - the home window is 500 ms;
  - a fresh login keeps 5 s;
  - the Fetch patterns hold the save-capable types and none of the static types;
  - a form post and a beacon on a Must not save case are still blocked;
  - the files are read once and read again after they change;
  - the next step's click waits for the outstanding picture, while a check does not;
  - the record waits for every picture;
  - a failed capture leaves no picture.
- **Real Edge (`browser_live`):** the Must not save live tests still pass.
- **Highlight:** the setting defaults on; off gives `highlight_ms` 0 for a watched run and the supervised browser; the tick box saves it (vitest).
- **Hand check owed:** a real unattended run of 3 cases, compared with the phase timings from 2.1.2.

# Auto Run: map the menus once with discovery

Date: 2026-10-09
Status: design approved in conversation; this file is the written spec for review.
Builds on: `docs/superpowers/specs/2026-10-08-autorun-discovery-design.md`.

## Goal

The assistant walks the menus of the modules the person names, once. It saves every screen it reaches as an area, so `/tcm:discover` starts knowing where everything is. Each screen's elements are recorded in the discovery map at the same time. The run never changes data.

## Owner decisions

| Question | Decision |
|---|---|
| Who walks the menus | The assistant, through a new `/tcm:map-menus` command, using discovery |
| Scope of a run | The modules the person names, on both the admin and self-service sides |
| Existing areas | Areas a person recorded are never changed. Areas an earlier mapping made are updated when their menu path changed, and the change is listed |

## Out of scope

- Mapping every module in one run.
- Exploring screen contents in depth: that stays per test case, in `/tcm:discover`.
- Filling in or submitting anything.

## 1. A mapping run

`start_autorun_discovery` gains `mapping: true` and `modules: [String]`.

- **Read only.** During a mapping run the app blocks every save request with the guard Must not save cases use (`save_guard`, the save words and `SAVE_METHODS`). A blocked request is logged and counted, never sent.
- **Elements recorded.** Every page read in the run records its elements in the discovery map, under the area being mapped, as discovery does today.
- **One browser.** A mapping run is a discovery session, so the Run buttons, Open browser, the title-bar pill and End discovery behave as they do for discovery.
- **A screen cap.** At most 150 areas are saved per run. The next `save_autorun_area` past the cap is refused with "This mapping run has saved 150 screens; end it and start another for the rest."

## 2. Who made each area

`ModulePath` gains `#[serde(default)] made_by: MadeBy`, where `MadeBy` is `Person` or `Mapping` and defaults to `Person`. Every area saved before this change loads as `Person` and is never touched by mapping.

`save_autorun_area`:

- An area saved in a mapping run is stored as `Mapping`. One saved by ordinary discovery stays as today: `Person`, because the person asked for it.
- **A name that exists as `Person`** is refused, as today.
- **A name that exists as `Mapping`**, saved in a mapping run, is updated after the replay check, if its clicks or its arrived path changed. Otherwise it is recorded as unchanged.

## 3. The summary

`end_autorun_discovery` on a mapping run returns, and logs at INFO with names and counts only:

- the screens added;
- the screens updated, each with its old and new path;
- the screens unchanged;
- the screens it could not reach, each with the reason;
- the number of save requests the guard blocked.

The last mapping run's summary is kept per project and shown in the Discovery dialog: when it ran, the modules, and each list. The paths are menu click names only, never addresses.

## 4. The command `/tcm:map-menus <modules>`

1. Start a mapping run with `mapping: true`, the modules, and an account the person names (or the project's default account).
2. For each module:
   - open it from the main menu;
   - walk every entry in its own sub-menu, the admin side first, then self-service;
   - save each screen reached as an area named "Module / Menu path", after checking the live areas list so nothing is duplicated;
   - go back home between branches.
3. Stop and report on a session-expired screen, an unexpected page, or the cap. Never fill in or submit a form.
4. End the run and report the summary.

The guide's discovery section gains one paragraph on mapping. `/tcm:discover` checks the areas list first, as it already does.

## 5. Errors and limits

- A screen whose clicks do not arrive on replay is listed as not reached, with the replay's reason.
- A save request blocked by the guard does not stop the run. It is counted, and the screen is still saved if it was reached.
- Advanced Features off: no mapping, as with discovery.
- No secrets, hosts or query strings in the summary or logs.
- No HTTP DELETE.

## 6. Testing

- **Rust:**
  - a mapping run blocks writes and counts them;
  - areas load as `Person` by default;
  - `save_autorun_area` stores `Mapping`, updates only a `Mapping` area and refuses a `Person` one;
  - the replay check on an update;
  - the summary lists;
  - the 150-screen cap.
- **Tools:** `start_autorun_discovery`'s new fields; the command text names the steps and the no-submit rule.
- **Frontend:** the Discovery dialog shows the last mapping summary.
- **Hand check owed:** one real mapping run of one module.

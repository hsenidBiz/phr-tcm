# Auto Run Simple mode - design

Agreed with the owner on 17 November 2026.

## Intent

Many people only want an assistant to write their Auto Run scripts and then run them. Today the Auto Run screen shows them everything the assistant itself works with: Discovery, Components, Fixtures, the sign-in recipe, replay, quirks and the raw script editor. That makes the screen look harder than the job is.

Simple mode shows what such a person needs and nothing more:
- pick cases;
- run them;
- see what failed;
- answer the assistant when it needs a decision.

Every feature keeps working underneath. Advanced mode is today's full screen, one switch away.

Two confusing pieces are fixed in both modes:
- **The divergence ruling buttons.** "The case is right" and "Rewrite the case" read as two ways of judging the case. The real question is whether the test case or the app is correct.
- **The open script card.** It lists assistant bookkeeping and long "unchecked" sentences about steps Auto Run does by itself.

## Goals
- A Simple / Advanced mode for the Auto Run screen. Simple is the default when Auto Run is switched on.
- Turning Auto Run on also turns on the recommended features: API Templates, Network tools and "Keep a video of failed cases".
- Simple hides advanced setup rows and advanced run/review controls, and never hides something that is stopping a run.
- The ruling buttons ask "which is right, the case or the app?".
- A cleaner open script card.

## Non-goals
- No feature is removed or switched off by Simple mode. Hidden parts keep working, and assistants keep all their tools.
- No change to what the assistant tools do or what they may access.
- No separate Simple screen (approach B was rejected).
- No change to the other screens (Run Tests, Playwright, API Templates and so on).
- No change to the ruling records or their behaviour; only their wording changes.

## Anti-goals (a failure even if it works)
- A Simple-mode person who cannot run because something hidden is broken, and is not told what.
- Existing Auto Run users finding parts of their screen gone after an update.
- Two diverging copies of the Auto Run screen.
- Simple mode turning on a feature someone switched off on purpose.

## Approaches considered
- **A. One saved mode setting; advanced parts hide themselves in Simple** (chosen by the owner). Small, one screen, and a hidden blocker still surfaces.
- B. A separate, lighter Auto Run screen for Simple. Rejected: two screens to keep in step, which will drift.
- C. No mode, only "Show more" folds everywhere. Rejected: not the clear switch the owner asked for, and the screen still feels busy.

## Design

### 1. The mode setting
- One app-wide setting, `autorun_mode`, stored with the other app settings (`app_settings.rs`). Its value is `simple` or `advanced`.
- It is per machine and user, like "Pause before each action"; not per environment.
- **Default:**
  - **New installs, and anyone switching Auto Run on for the first time:** `simple`.
  - **Existing installs that already have the Auto Run module on when this version first runs:** `advanced`. People who already use the full screen keep it.
  - Once the person picks a mode, it is never changed for them.
- **The switch:** a two-option segmented control in Auto Run's header row, labelled "Simple" and "Advanced". Changing it takes effect at once. It is not offered while an unattended run is going; it shows disabled with a title saying why.
- **Assistants:** an assistant can read the mode (it rides on the existing bridge context or a status answer), so it can say "switch to Advanced to see X". It cannot change it. The mode is a person's view choice.

### 2. Turning Auto Run on turns on the recommended features
When the Auto Run module goes from off to on (the Modules page is the only way; a fresh install starts with no modules), the same action also:
- switches **API Templates** on (it hangs under Auto Run);
- sets **"Keep a video of failed cases"** (`autorun_failed_video`) on;
- switches the **Network tools** on for every environment that has never had the switch set by a person. An environment whose switch the person set, on or off, keeps it.
- sets the mode to **Simple**, if no mode was ever picked.

Each of these applies only when the person has never set that switch themselves. A switch the person turned off stays off; the app keeps a record of "set by the person" for each one. Turning Auto Run off and on again does not re-enable something the person had switched off in between.

**Existing installs (coordinator's call).** Today nothing records who set these switches:
- turning the Network tools off removes its key;
- the video setting is a plain bool;
- API Templates being off is just its absence.

So the recommended features are applied only on a machine where Auto Run has never been on. A new app setting, `auto_run_ever_on`, records this.

It is worked out once at start-up, before Auto Run's own start-up code runs, because a release start-up creates the empty `autorun` folder on every machine. It becomes true when either:
- the Auto Run module is on; or
- the `autorun` data folder holds any file at all (empty folders do not count).

Switching Auto Run on sets it true, after the recommended features are applied. Once true, it never goes back. A machine that has used Auto Run before is treated as having made its own choices, and nothing is switched on for it. From this version on, every change to these three switches records "set by the person".

**Where it happens.** The Network tools switch lives in the webview's per-environment storage (`src/lib/networkTools.ts`), so the webview turns it on after a successful `modules_set(auto_run, true)`. Rust handles API Templates, the video setting and the mode. The "set by the person" record for the Network tools also lives in the webview.

Crystal Reports and other held-back modules are untouched.

**The Modules page.** It gets no new confirmation step (switching on has none today). After Auto Run is switched on, an inline note under its row says: "Also turned on API Templates, the Network tools and videos of failed cases. You can switch each off." It shows only when something was actually turned on.

### 3. What Simple hides

**Setup panel** (the collapsed summary `summaryLines` and the full rows in `SetupPanel.tsx`):

| Row | Simple |
|---|---|
| Environment | shown |
| Site address | shown |
| Sign-in | shown as one row with status only (ready / needs setting up). The recipe editor, Record sign-in, and recipe details are hidden. |
| Accounts | shown |
| Areas | shown |
| Discovery | hidden |
| Components | hidden |
| Test files | shown |
| Fixtures | hidden |
| Database | shown |

**Run and review side:**
- **Hidden:**
  - the **Replay to step** buttons in Past runs and the run review (`PastRuns.tsx`, `RunReview.tsx`);
  - **Import scripts**;
  - the script editing described under "View script" below.
  
  The Quirks list goes with the sign-in recipe editor, where it lives.
- **Kept:**
  - the run window (`ReplayPane.tsx`, which holds Start and the run options, including pause before each action and video);
  - the network-wait panel (the "company network is unreachable" pause with Stop);
  - results and the run review;
  - Past runs and the report;
  - divergence rulings, suspected defects, Raise bug, the video, Waiting for you, the bell, Execution order, and the Assistants card.

**View script (owner's answer).** In Simple, a case card's View script opens the readable script read-only: Runs as, Area, and the steps with their actions and checks. It has no field editing, no "Edit script" JSON editor and no Copy case. The footer has Close and a muted "Switch to Advanced to edit." In Advanced, the viewer is today's (`ScriptEditor.tsx`).

The builder lists every control they find that is not named here, with a proposed side, in the plan. The coordinator rules on those before implementation.

**The blocker rule:** a hidden part shows in Simple anyway when it is what stops a run. Each one gets its normal look plus one muted line: "Shown because a run needs it. Ask the assistant to sort it out, or switch to Advanced." A blocker here is any of:
- a setup row that the readiness check (`useAutoRunReadiness`) names as missing;
- the **Discovery row while a discovery holds the Auto Run browser**, so End discovery stays reachable (the run pill's note points at that row);
- the **Fixtures row while it shows a warning** (a failed fixture in the panel's "Needs attention" reasons);
- the Sign-in row's details when the sign-in itself is the blocker.

The readiness check widens to cover the discovery and fixture cases, with one function deciding "blocks a run" for both the header reasons and Simple's rule.

### 4. The ruling buttons (both modes)
Wording only. The rulings, records, assistant tools and undo stay as they are (`DivergenceMark.tsx`).
- **A question line before the buttons** for an open divergence that needs a ruling: "The case and the app disagree at step N. Which is right?"
- **Buttons:**
  - `case_stands`: **"The app is wrong"**. Confirm sentence: "The script keeps the case's value at step N, so the run fails there until the app is fixed."
  - `rewrite`: **"The app is right, update the case"**. Confirm sentence: "Azure DevOps opens on the case so you can update it; the case waits until its script is repaired to match."
  - `data_fixed` (data divergences only): **"I fixed the data"**. Confirm sentence unchanged in meaning: "You changed the environment's data to match the case; the case runs again."
- **Accessible names** follow the new labels (for example "The app is wrong for #12 step 3").
- **Everywhere else** the ruling words appear, use the same terms, including the assistant's ruling tool descriptions and guide text that a person sees quoted, and the How To Use guide:
  - the `Term` explanation for "ruling";
  - the "Ruled by the assistant" line;
  - the bell notices.
- **The tool's answer** to the assistant may keep its own internal names, but anything echoed to the person uses the new wording.

### 5. The open script card (both modes)
In `CaseCard.tsx`:
- **Header line:** one line instead of the facts list: "Script · 8 steps · runs as automation · Self Service › Insights › AI Insights".
- **Facts under the header:** the facts a person acts on stay as short lines: Must not save, Needs records, Preconditions, Assumed values and Invented values. The rule is that only facts that change what a run does or needs stay.
- **"Assistant notes":** a closed `<details>` with a count, holding Changes, Needs unchanged, Last repair, Checks given up, Page errors, Ignored page errors and Unexpected dialogs.
- **"Done by Auto Run":** steps that Auto Run itself performs before the script's first action show "Done by Auto Run" in place of "0 actions". Those steps are launching the app, signing in, and walking to the area.
  - **Source of truth:** a new optional per-step field in the script, `by_auto_run: true`, written by the assistant. The script guide tells it when to set it: a step whose whole expected result is launching, signing in, or reaching the area, which Auto Run does before step 1.
  - **Fallback for scripts saved before the field existed:** the leading run of steps (from step 1) with zero actions whose whole-step `unchecked` sentence is present counts as done by Auto Run.
  - Rust and TS share one rule and one fixture, like `unchecked`.
  - **Save check (coordinator's call):** `by_auto_run: true` stands in for the step's `unchecked` sentence in the save check (`floor.rs`). The save refuses `by_auto_run` on a step with actions, and on a step outside the leading run from step 1. Each refusal names the step in plain words.
- **"Not checked":** lists only the expectations of steps not done by Auto Run.
  - **Every consumer filters (coordinator's call):** Rust's `autorun::unchecked::expectations` applies the filter itself, so all of these agree:
    - the card;
    - the passed-case count;
    - the run report's "Unchecked" lines, its "N expectations unchecked" count and the Insights total;
    - the Playwright export's unchecked comments.
- **Steps list:** unchanged otherwise.

### 6. Assistants and guides
- **Script guide:**
  - document `by_auto_run`;
  - tell the assistant that people may be in Simple mode and that it should not send them to hidden screens, but offer to do the setup itself or say "switch to Advanced".
- **How To Use guide:**
  - a short section on Simple and Advanced;
  - the new ruling words;
  - retaken shots for the changed screens: Auto Run in Simple, the ruling row and the script card.
- **Changelog:** one end-user line.

## Constraints
- Every repository rule in `CLAUDE.md` applies:
  - theme tokens only, icons from `actionIcons.ts`, generated bindings, and the ui-consistency gate;
  - Rust tests in `tests/suite/`;
  - no URL or path in user text;
  - hidden features never named.
- The existing per-environment Network tools gating and its security reasoning stay. Only the default for never-set environments changes, and only at the moment Auto Run is switched on.
- **No regression for Advanced:** with `autorun_mode = advanced`, the Auto Run screen is exactly today's, apart from the both-modes changes in sections 4 and 5.

## Left to the builder
- **The header switch:** its exact placement and look within the header row, following existing segmented controls.
- **"Set by the person":** how the record is stored per switch.
- **Where the mode rides to assistants:** in the bridge context or a status tool answer.
- **The full inventory of Auto Run controls by mode** (section 3). Every unnamed control goes to the coordinator for a ruling before implementation.

## Tests (outline)
- **Mode defaults:** new install; first switch-on; an existing install with Auto Run on gets Advanced; a picked mode is never overwritten.
- **Switching Auto Run on:** turns on API Templates, video and Network tools for never-set environments. A person-set off stays off. Off then on again does not re-enable a person-set off.
- **Simple hides:** the hidden rows and controls are hidden in Simple, and all of them show in Advanced.
- **The blocker rule:** a hidden row that blocks a run shows with its note.
- **Ruling labels:** new labels, question line and confirm sentences; the same rulings are sent.
- **Script card:**
  - the header line;
  - Assistant notes is closed by default with its count;
  - "Done by Auto Run" comes from the field and from the fallback (shared fixture, Rust and TS);
  - "Not checked" is filtered, and so is the passed-case count.
- **jsdom limits:** cannot prove layout. The owner's hand check covers Simple at a real window size.

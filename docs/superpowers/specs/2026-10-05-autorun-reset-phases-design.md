# Auto Run: shared-state changes, reset phases and Auto Run's own order (part 6)

Design agreed with the owner on 2026-10-05.

Some test cases change shared state, for example publishing a cycle. Other
cases only work while that change has not happened, or after it is reverted.
The owner wants:

- such cases marked, but only where necessary;
- a run of a set that stops where the environment must be reset, then carries
  on;
- the cases put in a suggested execution order.

Owner decisions:

- **Marking:** cases are marked with named changes, not with links to other
  cases.
- **At a reset point:** an unattended run pauses and asks, and Continue
  carries on in the same run.
- **Order:** Auto Run keeps its own execution order, separate from Run Tests'
  PBI order.
- **Who marks:** the assistant marks cases when it writes scripts, and the
  script editor shows and edits the marks.
- **When:** this is built after part 4 (replay to step N) and the startup
  speed-up, and before part 5.
- **Release:** it ships as its own beta.

## 1. Marking a script

`CaseScript` gains two optional lists. Each is serialised only when it is not
empty, so old scripts load unchanged:

- `changes: [name]`: this case leaves something changed for the cases after
  it.
- `needs_unchanged: [name]`: this case needs that thing not yet changed (or
  reverted).

A name is a short phrase, for example `"cycle published"` or `"appraisal
submitted for A001"`.

Rules for names:

- **Comparison:** names are compared trimmed and case-insensitively.
- **Spelling:** the first spelling seen is kept for display.
- **Limits:** a name is 1 to 60 characters, and each list holds at most 10
  names.
- **Refusals at save:**
  - `changes: "<name>" is longer than 60 characters`
  - `changes holds more than 10 names`, and the same two for `needs_unchanged`
  - an empty name: `changes: a name cannot be empty`

  These apply to the editor's save, the assistant's save and import.
- **The same name in both lists is allowed.** A case that needs X unchanged
  and then changes X is the usual shape, for example "publish the cycle".

Repairs may change these lists. They affect order, not safety, so there is no
repair rule.

### The guide

The guide (`autorun/guide.rs`) says to mark a case only when it really leaves
shared state that another case depends on. Re-runnable cases that clean up
after themselves carry no marks.

It gives the publish example:

- the case that publishes has `changes: ["cycle published"]`;
- each case that edits the draft cycle has `needs_unchanged: ["cycle
  published"]`.

It also says to reuse one name for one change across the set.

### The script editor

Under the steps, the script editor shows two rows, "Changes" and "Needs
unchanged". Each name is a chip with a remove button, and each row has a text
box to add a name. The editor's save validates them as above.

## 2. The plan: order and phases

### The suggested order

For a selection of cases (Auto Run's Run selected or Run unattended), the app
works out a suggested order:

1. Start from the cases in list order.
2. Move every case that needs X unchanged ahead of every case that changes
   X. Keep list order wherever no mark forces a move, as a stable
   topological sort over the constraints "a needer of X runs before a changer
   of X". A case that both needs and changes X stays after the other needers
   of X and before the other changers of X.
3. When the constraints form a cycle (A changes X and needs Y unchanged,
   while B changes Y and needs X unchanged), break it as late as possible in
   list order. The break becomes a reset.

### Phases

Walk the order. A run goes into a new phase before a case that needs X
unchanged when X has already been changed in the current phase.

Each boundary names:

- the names to revert;
- for each name, the case or cases that changed it.

### Auto Run's own order

The order is kept per PBI on this machine, in `orders/<pbi id>.json` in the
Auto Run store, as a list of case ids.

**Where it comes from:**
- If none is saved, the suggested order is used.
- **The person's way in:** an "Execution order" dialog on the Test cases
  tab lists the cases in the current order, with drag handles and the
  keyboard (up/down buttons, as the existing ordering dialogs do).
- **The assistant's way in:** a new MCP tool, `set_autorun_order { pbi_id,
  case_ids }`. Ids not in the PBI are refused with `case <id> is not in PBI
  <pbi>`, and any listed case that has no script is warned about.

**How a saved order is used:**
- A saved order is used from then on. Selected cases missing from it go at
  the end, in list order.
- Phases are worked out from the saved order the same way.
- If a saved order needs more resets than the suggested one, the dialog says
  so: `this order needs <n> resets; the suggested order needs <m>`. It offers
  "Use suggested order".
- "Use suggested order" clears the saved order.

### Before a run starts

The run dialog shows the plan:

- **Phases:** each one, with how many cases it has.
- **Reset points:** at each one, `Reset: revert "<name>" (changed by #<id>
  <title>)`.

Where there are no marks, there is one phase and nothing extra is shown.

## 3. A reset point during a run

### Unattended runs

At a boundary, after the last case of a phase has finished and its browser
has closed, the run pauses.

**What the panel shows:**
- The heading `Reset needed`.
- Each name to revert, with the case or cases that changed it.
- The cases still to run.

**What the person can do:**
- **Continue:** the next phase runs, in the same run.
- **Stop:** the run ends there, and the remaining cases are recorded as not
  run, with the reason `not run: the run stopped at a reset point`.

**The run record:**
- It gains `resets: [{ before_case_id, names, changed_by, waited_ms,
  outcome: "continued" | "stopped" }]`.
- This is serialised only when it is not empty.
- Past runs and the HTML report show a `Reset: revert "<name>"` line between
  the cases where it happened, with "continued" or "stopped".

**While paused:**
- The pause has no time limit.
- If the app is closed, the run ends as if Stop was pressed.
- An unattended run started by the person only pauses when the person can
  see the panel. Unattended runs are always started from the app.

### The supervised pane

Between cases, the same panel shows before the next case's browser is used.
Continue and Stop work in the same way.

### Not here

The assistant's replay (part 4) and tries never pause for resets. They run
one case.

## 4. Out of scope

- **Automatic resets.** A reset point is carried out by a person. Part 5's
  fixtures may later offer "run this fixture to reset".
- **Run Tests' PBI run order.** It is unchanged and separate.

## 5. Testing

- **Rust tests** (in `tests/suite` only):
  - name validation and refusals;
  - old scripts loading unchanged;
  - the planner:
    - no marks gives list order and one phase;
    - one change with needers before and after;
    - a case that needs and changes the same name;
    - two names;
    - a cycle;
    - a saved order that needs extra resets, and the reported counts;
    - selected cases missing from the saved order;
  - the order store and the `set_autorun_order` refusals;
  - with fake browsers, the unattended run:
    - pausing at a boundary;
    - Continue running the next phase;
    - Stop recording the rest as not run;
    - the `resets` record.
- **vitest:**
  - the editor rows;
  - the Execution order dialog (drag and keyboard, the reset counts, Use
    suggested order);
  - the run dialog's plan;
  - the Reset needed panel (Continue and Stop) in the unattended pane and the
    supervised pane;
  - Past runs and the report showing the reset line.

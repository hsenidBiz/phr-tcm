# Auto Run: components, reusable steps for common widgets

Date: 2026-10-08
Status: design approved in conversation (approach A, parts 1 to 3); this file is the written spec for review.
Builds on: `docs/superpowers/specs/2026-10-08-autorun-discovery-design.md` (discovery, the map, the save check).

## Goal

A widget or short flow is worked out once on the live app, saved as a named component with inputs, and used by any script with a single step. Fixing the component fixes every script that uses it. This mirrors `PHR-PLAYWRIGHT-AUTOMATION/src/components/` (`Dialog`, `AppShell`, `Toast`), done as data rather than code.

## Owner decisions

| Question | Decision |
|---|---|
| Scope | Single widgets (date picker, slider, searchable dropdown) and short multi-element flows (confirm the visible dialog, close a toast, open a row's actions menu) |
| Updates | Scripts reference components by name; the runner expands them when it runs, so a fix reaches every script; a change passes the same gate as a repair |
| Authoring | The assistant makes components during discovery, tried live; the guide lists them and tells the assistant to use them; the person can view and remove them; nothing forces their use |
| Approach | A: a `use_component` step with named inputs, of kind `text` or `target` |

## Out of scope

Components inside components, loops, conditions beyond `when_visible`, formatting functions, forcing their use, an approval step, components in delete templates.

## 1. The component

Stored per project at `<autorun root>/projects/<slug>-components.json`, written atomically under one lock, like the map.

```
Component {
  name: String,            // unique per project, compared like area names (case and spaces folded)
  description: String,     // one line: what it does, when to use it
  inputs: Vec<ComponentInput>,
  actions: Vec<Action>,    // ordinary Auto Run actions
  tried_at: u64,           // when it was last tried live in discovery
  tried_area: String,      // the area it was tried in
  version: u32,            // starts at 1, +1 per accepted change
  changes: u32,            // accepted changes, capped at 3 before the guide says stop and report
}
ComponentInput { name: String, kind: "text" | "target", description: String }
```

- A `text` input appears in action values as `{{name}}`.
- A `target` input appears where a locator goes, as `{"input": "name"}`, and may also be one link of a chain.

## 2. In a script

A step may hold, alongside ordinary actions:

```
{ "kind": "use_component", "component": "Pick a date",
  "inputs": { "field": {"role": "textbox", "name": "Leave start"}, "date": "2026-11-03" } }
```

When a script runs:

- The runner looks the component up by name and puts the inputs into its actions. Target inputs go into locators and chains, text inputs into values. The actions then run in order as part of the step.
- The step's outcome names the component, then the lines of its own actions.
- A missing component fails the step with "<name> is not saved in this project". A missing input fails it with "<name> needs <input>". Neither is ever skipped.
- The run record stores the component's name and version for each use.

Tests and fixtures may use components. Delete templates may not.

## 3. Checks

**Saving a component** (`save_autorun_component {name, description, inputs, actions, why?}`):

- Every locator in its actions must be in the discovery map, using the same rules as the script save check. A target-input placeholder is exempt.
- Every input the actions use must be declared, and every declared input must be used.
- No sign-in action, and no `{{username}}`/`{{password}}`-style placeholders.
- It must have been tried in the current discovery session, through `discover_autorun_action` with a `use_component` action.

**Saving a script that uses components:**

- The component must exist.
- Every declared input must be given, with the right kind: a string for `text`, a locator for `target`.
- A target input's locator goes through the seen check like any other locator in the script. The script-typed and test-case exceptions apply to it as they do elsewhere.

**Changing a component** (saving one whose name exists):

- `why` is required.
- It must never weaken. It may not remove a check action the old version had, or turn a check into a non-check. This is the same rule as script repairs.
- It must be tried live again in discovery before the save.
- After 3 accepted changes the save still works, but the guide tells the assistant to stop and report to the person instead.
- The version goes up by one, and the old run records keep their own version number.

**Removing** (`remove_autorun_component {name}`) is refused while any saved script uses the component, naming those scripts.

## 4. What the assistant is told

- **The guide** gains a "Components" section:
  - what components are, and the two input kinds, with an example;
  - when to make one: the first time a widget or short flow will be needed more than once;
  - to use an existing component instead of repeating its actions.
- **The live guide** lists this project's components with their inputs and descriptions.
- **`/tcm:discover`** checks the list before working out a widget, and saves a new component once it has tried one live.
- **`/tcm:heal`**: a failure inside a component is fixed in the component, not the script.
- **New tools:** `save_autorun_component` and `remove_autorun_component`, on the Auto Run gate. `discover_autorun_action` accepts `use_component`.
- The banned phrases "read the source", "application's source" and "source-derived" stay banned.

## 5. In the app

**A Components row** in Auto Run's Setup panel, under Discovery. Its summary reads "N components" or "None yet". It opens a dialog that lists, for each component:

- name, description, and its inputs with their kinds;
- the area it was tried in, and when;
- how many scripts use it, with their case ids;
- its changes, shown as "N of 3";
- Remove, enabled only when no script uses it. When it is disabled, its title names the scripts that use it.

**In runs, Past runs and the review:** a `use_component` step shows the component name with its own action lines indented under it.

**How To Use** gains a Components section.

## 6. Errors and limits

- A damaged components file: the error names `projects/<slug>-components.json`. **Reset** moves it aside to `<name>.corrupt-<time>.json`, never deleting it. Scripts that use components then fail with "<name> is not saved in this project" until the components are saved again.
- A failure inside a component names the component, the action, and its text inputs. Typed-value masking is unchanged.
- No secrets, hosts or query strings in the components file or logs. No passwords in inputs.
- No HTTP DELETE to Azure DevOps.
- Advanced Features off: no component tools.

## 7. Testing

- **Rust:**
  - the store: load, save, the lock, a corrupt file and Reset;
  - expansion: text inputs, target inputs in locators and chains, a missing component, a missing input;
  - the component save checks: unseen locators, undeclared or unused inputs, never tried live, sign-in refused;
  - the script checks: unknown component, wrong-kind input, an unseen target input;
  - the change gate: `why` required, never weakened, the version going up;
  - removal refused while in use;
  - the version recorded in runs;
  - the tools and their gating;
  - the guide's Components section and live list.
- **Frontend:** the Components row and dialog, Remove enabled and disabled, the step lines in a run and in the review.
- **How To Use:** its guard tests pass.
- **Owed by hand:** a real date-picker component made during discovery and used by two scripts.

# Auto Run: things assistants get wrong

Mistakes an assistant made while driving Auto Run through the MCP tools, what
they cost, and the app change that would stop the next one. Each entry is a
note for whoever changes the app; the tool text and the guide are what an
assistant actually reads, so a fix belongs there, not only here.

## Searching the whole disk for a screenshot

**What happened (2026-10-09, `/tcm:map-menus` on Performance & Evaluation).**
A failed or tried action answers `(picture: shot-1791553284989-000002.jpg)` -
a bare file name. To look at the picture, the agent searched for it:

    find / -name "shot-1791553284989-000002.jpg" 2>/dev/null | head -2
    find "$APPDATA" "$LOCALAPPDATA" -name "shot-1791553284989*" 2>/dev/null | head

`find /` walks every drive. It ran past the shell's 2-minute limit, was moved
to the background, and the mapping run sat waiting on it with nothing saved.

**Where the pictures always are.**
`%APPDATA%\com.avinalwis.testcasemanager.v2\autorun\shots\<name>.jpg`
(the store root's `shots` folder). Nothing else writes them.

**Fix in the app.**
1. Every answer that names a picture gives its full path, not the bare name:
   - `src-tauri/src/ai_bridge.rs` - the try / discovery answer
     (`text.push_str(&format!(" (picture: {shot})"))`).
   - `src-tauri/src/autorun/failures.rs` - `get_autorun_failures`
     (`out.push(format!("  picture: {shot}"))`).
   Same for any other place that prints `shot-…jpg`.
2. The guide (`src-tauri/src/autorun/guide.rs`, "Seeing the page") says, in
   one place: pictures are at that path; open one directly with the file
   reader; never search the disk for it; and prefer `get_autorun_page` (with
   a `limit`) or `probe_autorun_locator`, which answer in text and need no
   picture at all.
3. A test in `tests/suite/` that a failure line and a try answer carry an
   absolute path that exists.

**Until then, for screenshots (for an assistant reading this).**
Do not search for a screenshot. Read it at the path above, or skip the
picture and read the page as text with `get_autorun_page` /
`probe_autorun_locator`. Never run `find /` (or any whole-drive search) from
the shell: it outlives the command timeout and stalls the work.

## Mapped areas that depend on the sidebar being open

**What happened (same mapping run, 2026-10-09).** `save_autorun_area`'s check
replayed the clicks from the browser's current menu state, not from a fresh
home page. PeoplesHR's left menu remembers whether it is open (in
localStorage), and clicking "Talent" when its section is already expanded
collapses it. So:

- the first save of the Definition Wizard screen was refused ("text "Talent"
  is outside the visible part of the page") because the sidebar was closed;
- the agent opened the sidebar with the hamburger toggle before each later
  save, and those saves passed - but the toggle is not in the saved clicks,
  so the five saved paths only work when a run starts with the sidebar open;
- the refused attempt is still counted as "unreached" in the summary even
  though the same screen was then saved under another name.

**Fix in the app.**
1. Replay a `save_autorun_area` check (and a run's trip to an area) from a
   known start: the home page with the menu in a fixed state - e.g. run the
   recipe's `after_sign_in` menu-open step (it opens the menu with the toggle
   and `:not(.active)`) before the first click, every time.
2. Or record the toggle as the first click of every mapped path, guarded so it
   only opens a closed menu.
3. Drop an unreached entry from the mapping summary once the same screen is
   saved under another name in the run, or say so next to it.

**Until then.** Before saving an area, make sure the menu is open the way a
fresh run leaves it, and expect the first run of a mapped area to fail at
"Talent" if a person left the menu closed.

## Script and component saves refused as "never seen on the live app"

**What happened (2026-10-09, PBI 135592 scripts).** Counted from the agents'
transcripts:

| Agent | `save_autorun_component` ok / refused | `save_autorun_script` ok / refused |
|---|---|---|
| Cycle Setup (finished) | 5 / 0 | 13 / 9 |
| Evaluation Rules (first hour) | 1 / 4 | 1 / 4 |

Almost every refusal is the sighting check (`Step N: <locator> was never seen
on the live app`). It compares locator text literally, so locators that are
legitimately built from data never match a sighting. The refusals fall into
these groups.

### 1. Placeholders inside locators and component inputs

Refused, verbatim:

    Step 3: button[aria-label^="Edit"] in div[data-cycle-id="{{fixture.pc-draft-before-evaluators.cycle_id}}"] was never seen on the live app.
    Step 3: button[aria-label^="Edit"] in div[data-cycle-id="{{setup.cycle_id}}"] was never seen on the live app.
    Step 3: Edit cycle by id got a placeholder as id

The page was seen with `data-cycle-id="10066"` etc.; a script that opens a
fixture or setup draft can only name it through a placeholder, so it can never
pass. This blocks every script that starts from a fixture draft.

**Fix.** In the sighting check, treat `{{fixture.*}}`, `{{setup.*}}`,
`{{prefix}}` and `{{now:...}}` as wildcards and match them against the seen
value's shape (e.g. `data-cycle-id="<digits>"`). Let `use_component` inputs
carry a placeholder at save time and check the resolved value at run time.

### 2. State or filter added to a seen selector

    Step 5: #er-goals-checkbox input:checked was never seen on the live app.
    Action 2: .phr-mc-card:not([data-cycle-name*="Annual Performance Review"]) was never seen on the live app.

The base element (`#er-goals-checkbox input`, `.phr-mc-card`) had been seen;
the selector with `:checked` / `:not(...)` had not.

**Fix.** Match on the base selector with state pseudo-classes removed
(`:checked`, `:disabled`, `:enabled`, `:focus`, `:not(...)`, `:has(...)` when
its inner part was seen), or check a CSS selector by running it against the
stored snapshots of the area instead of comparing its text. The guide should
also point at `expect_attribute` (`checked`, `aria-checked`, `disabled`) as
the way to check state.

### 3. Text produced by the test's own data

    Step 5: text "240.0 KB" was never seen on the live app.          (size of an uploaded Test file)
    Step 7: button "Download policy.docx" was never seen on the live app.   (a Test file's name)
    Step 4: button "15/01/2027" was never seen on the live app.       (a date-picker day)

Typed text is already exempt; text that comes from an uploaded Test file or a
picked date is not.

**Fix.** Exempt, like typed text: the names of Test files the script uploads
(and text containing them), and their displayed sizes; a date-picker day
button whose name is a date the script picked or typed (or any `dd/mm/yyyy`
inside a date picker that was seen).

### 4. Near misses on accessible names

    Step 3: text "Step 1 of 9 — Cycle Setup" was never seen on the live app.
    Step 3: progressbar "Step 1 of 9 — Cycle Setup" was never seen on the live app.
    Step 5: progressbar "Step 2 of 9 — Eval Rules" was never seen on the live app.

The progress title was seen; the stored name differed (dash character /
spacing, or the role).

**Fix.** Normalise whitespace and dashes (`—`, `–`, `-`) and case before
comparing names. In the refusal, name the closest seen locator ("did you
mean progressbar \"Step 1 of 9 – Cycle Setup\"?"), so the agent fixes it in
one try instead of re-exploring.

### 5. Elements that only exist in some states

    Action 2: button "2 page" was never seen on the live app.

Pagination appears only when there are more than ten cycles; a component that
pages the list was written before page 2 was ever visible in discovery.

**Fix.** On a refusal while discovery is open, offer to probe the refused
locator on the current page and record the sighting if it matches (one call
instead of probe + save again).

### 6. Agent mistakes (guide wording, not code)

    case 136467: step 5 says it is unchecked but has a check - drop one or the other
    409: Try the component live in discovery first: run a use_component of it with discover_autorun_action, sending this component as its draft, and save it once that works, unchanged.

**Fix in the guide (`autorun/guide.rs`, "Saving it" and "Components").** Say
plainly, before the save examples:
- probe every final locator exactly as written immediately before saving;
- a component must be tried with `use_component` + `draft` before
  `save_autorun_component`, and saved unchanged after that try;
- a step is either `unchecked` (no checks at all) or checked - never both;
- open a fixture draft by its unique name (fixture names carry a timestamp)
  until placeholders are accepted in locators.

**Until then (for an assistant reading this).** Open fixture drafts by their
unique AUTOTEST name, not by id; check state with `expect_attribute` on the
seen element instead of `:checked`; probe each final locator exactly as
written right before saving; try every component with `use_component` before
saving it.

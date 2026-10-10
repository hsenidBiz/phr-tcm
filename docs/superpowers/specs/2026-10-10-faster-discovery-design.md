# Auto Run: faster discovery and script writing

Date: 2026-10-10
Status: draft for the owner's review.
Source: `speeding-up-discovery.md`, items 1, 3, 4 and 10, agreed 2026-10-10. Item 5 (one discovery browser per account) is left for later. Item 6 (open a record by id) is dropped.

## Goal

An assistant writes a page's scripts from one look at each screen, with almost no refused saves:
- reading a screen counts as seeing it;
- a refused save names every problem at once;
- routine walks take one call;
- a repeat job starts from a short rule sheet instead of the full guide.

The gate does not change. A script still uses only what was really on the live page.

## 1. Reading a screen records what it shows

- **Today.** `get_autorun_page` returns the page as text but records nothing. Only `discover_autorun_action` and the area save record sightings, so assistants click around just to get locators past the check.
- **New.** During a discovery, `get_autorun_page` records every element it lists as seen.
  - The sightings go under the discovery's current area, keyed and kept exactly as an action's page read is (the `sightings` list from 2.1.3-beta.5).
  - This includes an open dialog's contents and same-origin frames, as the page text already shows them.
- **Limits.**
  - Only while a discovery is open and on the application's own origins. Outside a discovery, `get_autorun_page` behaves as today.
  - When the read is cut short by `limit`, only the lines it returned are recorded.
  - Hidden elements are not recorded, because the page text never lists them.
- **Answer.** The answer ends with one line: "Recorded N elements as seen on <area>."
- **Area.** If the discovery has no current area yet, nothing is recorded, and the line says so, telling the assistant to save or name an area first.

## 2. A refused save lists every problem

- **Today.** A script or component save stops at the first unseen locator. Three problems cost three saves.
- **New.** The save checks every step and every action, and refuses once, listing every unseen locator in step order. Each entry carries its own "did you mean" hint, same role only.
- **Also during a discovery.** The record-on-page check (2.1.3-beta.2) runs over the whole list, records what it finds, and checks the save again once. The answer lists what was recorded and what is still refused.
- **Dry run.** `save_autorun_script` and `save_autorun_component` accept `"dry_run": true`. The save runs every check, the seen check included, and answers what would be refused, but writes nothing. It never records sightings.
- **Size.** The refusal is capped at 50 entries, with "and N more" after that.

## 3. Several actions in one call

- **New tool:** `discover_autorun_actions` takes `{ actions: [...], stop_on_failure: true }`.
  - It runs the actions in order in the discovery browser, exactly as the same number of `discover_autorun_action` calls would. The same rules apply: Must not save blocking, mapping-run blocking, the sightings recorded per action, `use_component` with `draft`, and the no-credentials rule.
  - The answer gives one line per action (ok or failed, with the reason) and the page text only once, after the last action that ran.
  - At most 20 actions per call.
  - It stops at the first failure unless `stop_on_failure` is `false`.
- **Behaviour.** Each action's own page read still happens and still records sightings. Only the answer is shorter.
- **Gate.** It is on the Auto Run gate, like the other discovery tools.

## 4. A short rule sheet for repeat work

- **Guide.** A "Quick rules" section at the very top of the guide, about 40 lines. It covers:
  - never read the application's code;
  - the save rules: probe each final locator as written, try a component before saving it, `unchecked` or checked but never both, `expect_attribute` for state, and where placeholders may stand;
  - where pictures are;
  - reading a page counts as seeing it;
  - the dry-run save;
  - the batch tool;
  - ending discovery;
  - Must not save;
  - stop on a cap or an expired session.
- **Tool.** `get_autorun_guide` gains `"quick": true`, which answers only that section plus the project's live sections (areas, accounts, components). The `/tcm:discover`, `/tcm:map-menus` and `/tcm:heal` commands say: read the quick guide first, and the full guide when something it does not cover comes up.
- **Wording.** The banned phrases stay banned. Hidden features are never named.

## Errors and limits

- Nothing that the seen check accepts or refuses changes, except that reading a page now records sightings.
- No hosts or query strings in logs or answers. No HTTP DELETE. No em or en dashes.
- Advanced Features off means none of this, as with discovery.

## Testing

- **Rust:**
  - `get_autorun_page` records sightings only during a discovery, only for lines returned, and only with a current area;
  - a save refused for three unseen locators lists all three, each with its hint;
  - the cap of 50;
  - a dry run writes nothing and records nothing;
  - the batch runs in order, stops on the first failure, answers the page once, and records what each action read;
  - the batch limit of 20;
  - the quick guide holds the rules and the live sections, and the commands point at it.
- **Hand check owed:** write one page's scripts with the new tools and count the refused saves.

# Playwright tab: follow the repo's folders, whatever their depth

Date: 2026-10-15. Status: design, from the conversation with Avin; awaiting his review.

## Why

The team's Playwright repo has moved its features one level deeper where a module holds
several areas of its own:

    suites/<country>/<side>/<module>/[<group>/]<feature>/{specs,pages,data,test-cases}/
    e.g. suites/sl/admin/performance/definition-wizard/competencies/specs/

TCM hardcodes the old four-level shape (`<country>/<side>/<module>/<feature>`) in the Runner,
the Overview, Sync, Export and Finish in repo, so grouped features vanish or land in the wrong
place. Avin wants TCM to handle this layout **and any folder structure the repo defines next**,
without TCM changing again.

## Decided

**TCM follows the folders; it does not ask the repo** (Avin chose this over adding a layout
command to the repo). TCM keeps only the repo's vocabulary, never a depth:

- A **feature** is any folder under `suites/` that directly holds `specs/`, `test-cases/`,
  `pages/` or `data/`. Its **path** is the list of folders from `suites/` down to it, of any
  length. `_generated`, dot-folders and `node_modules` are never part of the tree.
- The **first folder** is the country (it names the Playwright project, uppercased, as today),
  except `common-components`, the shared tree, which runs under SL as today.
- The **navigation key** is the last two folders of the path - the repo's own rule
  (`navigationKeyFor` in `cli/paths/suite-path.mjs`): `performance/rating-methods`,
  `definition-wizard/competencies`, `common-components/employee-search`.
- A feature's **test-case file** is `suites/<path>/test-cases/<feature>.md`, where `<feature>`
  is the path's last folder.
- TCM no longer checks that the second folder is `admin` or `self`, or that the country is `sl`.

## What changes for the person using it

- **Runner:** the tree shows each folder of the path as its own level, so a grouped feature sits
  under its group (Country > Side > Module > Group > Feature > spec, and one level less where
  there is no group). Grouped tests can be ticked and run, and the count check counts them.
  Common components show as today.
- **Overview:** one row per feature, with its folder path (e.g. `sl / admin / performance /
  definition-wizard`) and the feature, in place of the fixed Country, Side, Module, Feature
  columns. Common components are listed too.
- **Sync:** a case whose section sits in a grouped test-case file is found as in the repo, so
  it is Current or Out of date, never Not in repo, and Update rewrites it where it is.
- **Export:** a case's area is placed by choosing one of the repo's feature folders from a list
  (filtered as you type), or by typing a new path such as
  `sl/admin/performance/definition-wizard/new-area` (folders in lower-case kebab case). The
  suggestion is the existing feature whose name matches the area. Placements already saved as
  side/module/feature keep working (read as `sl/<side>/<module>/<feature>`).
- **Finish in repo:** accepts a test-case file at any depth, and checks and names the
  navigation entry by the last two folders.

## Non-goals

- No change to what Export writes (the test-case file, the raw spec in `_generated`, the index)
  or to the Accounts tab.
- No new repo command, and no layout file in the repo.

## Anti-goals

- A grouped or deeper feature missing from any tab, or counted wrong by the run's count check.
- Export writing a test-case file onto a group folder (the old `<module>/<group>` guess).
- A saved placement from before this change being lost.

## My calls (Claude), for review

- Folder names stay the repo's vocabulary (`specs`, `test-cases`, `pages`, `data`,
  `_generated`, `common-components`); renaming those in the repo would still need TCM to change.
- A feature folder with only `pages/` or `data/` (such as `common-components/employee-search`)
  is listed in the Overview and offered for placement, with no specs and no cases.
- **Repo side, for Avin:** the repo's `.claude/commands/gen-test.md` still derives the spec
  folder as `sl/<side>/<module>/<feature>`; the handoff prompt will name the grouped path, so
  that command needs the `[<group>/]` form too. TCM does not edit the repo.

## Testing

- The fixture clone gains a grouped feature and a deeper one; Runner, Overview, presence, Sync,
  Export and Finish in repo each find and place them, and the four-level and common-components
  cases keep working.
- An old saved placement reads back as its path.
- The navigation key is the last two folders at every depth.

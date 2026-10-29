# Per-environment Auto Run data - design

## Intent

Environments can run different versions of the app. A script, a menu path, or an API template written against one environment's version can be wrong for another. So each environment gets its own Auto Run scripts and everything that describes its version of the app, and these change independently.

Today all of it is shared, except accounts, saved sign-ins and proposed accounts, which are already kept per environment.

**The owner's workflow.** The owner described how environments are used:

- The **developer environment** is for trying things out during development. It has read/write access to the database. Its scripts are working copies, for testing only.
- Once development is done, the app moves to the **automation environment**, where it is tested again.
- The scripts written in the automation environment are the final ones. They are **always** the ones exported to Playwright.

The developer environment's scripts must therefore never reach the Playwright repo. The usual path is to write in the developer environment, then copy into the automation environment once the app has moved there, adjusting as needed.

## The owner's decisions

- **Scripts are fully separate copies per environment.** Different steps are allowed, not just different data.
- **Everything is per environment from now on.** Shared is no longer the default.
- **Existing data goes into every environment.** At the update, today's data is copied into every environment that exists. Copies then drift apart as people edit them.
- **Storage is a folder per environment (approach 1).**

## Approaches considered

1. **A folder per environment (chosen).**
   - Each environment's Auto Run data lives in its own folder.
   - Switching environment switches the folder every store reads and writes.
   - Most store code is unchanged, because it already takes a root.
   - An environment can only ever see its own folder.
   - Cost: disk space per environment, but this is small text data.
2. **Shared files with an environment tag on each entry (rejected).**
   - Every reader and writer must filter by environment.
   - One missed filter leaks another environment's script into a run, which is exactly what this work is meant to prevent.
3. **A database (rejected).** It means rewriting all Auto Run storage, with no benefit here.

## What moves per environment

Everything that describes one version of the app, or a run against it. These are the owner's calls where noted.

| Moves per environment | Notes |
| --- | --- |
| Scripts | Including deleted scripts kept for 30 days. |
| Areas | Menu paths and their variants. |
| Components | |
| Quirks | |
| Run order, marks, suspected defects, divergences, validation rules | Any per-case state that lives beside scripts. The builder checks each store under the Auto Run root. |
| Discovery map | |
| Saved setups | |
| Explore changes | |
| API templates, fixtures and flows | They hold data and call that environment's API. |
| Local runs, run results, reports and run downloads | Results from Staging and Dev must never mix. My call. |

## What stays shared

| Stays shared | Notes |
| --- | --- |
| The environments list (`environments.json`) | |
| Accounts, sessions and proposals | Already keyed by environment id. |
| Test files that cases upload | Sample spreadsheets and documents don't depend on the app version. My call. |
| App-wide settings | Anything that is about the machine or the person, not about an app version. The builder lists these. |

## Layout

The data moves from `<appdata>/autorun/...` to the following structure. The exact folder names are left to the builder.

- `<appdata>/autorun/` (the shared root) holds the environments list, accounts, sessions, proposals, test files and other shared items.
- `<appdata>/autorun/envs/<env-id>/` (the environment root) holds everything in the per-environment table above, laid out exactly as today's root lays it out. Existing store code therefore works unchanged, given the right root.

The store gains two lookups:

- the shared root;
- the active environment's root.

Every call site that reads or writes per-environment data uses the active environment's root. The builder goes through each `configured_root()` caller and classifies it. A test pins that classification, for example a list of the store modules and the root each one uses, so that a new store can't silently land in the wrong place.

## The one-time copy at update

1. On first start of the new version, if `autorun/envs/` does not exist yet, copy every per-environment item from the old root into `envs/<id>/` for every environment in the list.
2. If there are no environments yet, keep the copy as `envs/_seed/`. An environment created later starts from the seed. A machine that never set up environments gets its data as soon as it creates one.
3. Leave the originals in place, untouched, as a backup. They are never read again. Delete them only in a later release, after the owner says so.
4. The copy is all or nothing. Copy into a temporary folder, then rename it into place. A crash part-way leaves the old layout in charge and the copy is retried on the next start.
5. Log one line with the number of environments and items copied, and no paths.

## New environments

A new environment starts with an empty folder, and the person can **copy from another environment**:

- **Whole environment.** "Copy Auto Run data from…" on the environment's settings copies the source environment's per-environment folder into the new one. It is offered while the new environment is empty. On a non-empty one it asks first and replaces the contents, keeping the old contents as a backup folder.
- **One case.** On a case's script, "Copy to environment…" copies that script, and any components it uses that the target lacks, into another environment. If the target already has a script for that case, it asks before replacing it.

A freshly created environment is seeded from `_seed` if `_seed` exists and no environment has been created since the update. Otherwise it starts empty. My call.

## Removing an environment

Today, removing an environment deletes its accounts. Its Auto Run folder is now kept for 30 days in a removed-environments area and then purged, like deleted scripts. It is never deleted at once. My call: these are hours of work. See "Removed environments" below for how the work can be recovered.

Removing the Playwright export environment is refused until another environment is named as the export environment.

## Switching environment

- Switching is refused while a run, a replay, a discovery or explore session, or a held browser lease is active in the current environment. The person is told what to stop first.
- Everything already open reloads from the new environment's folder. This includes the Auto Run screen, the API Templates screen, Playwright export and pending lists.
- The assistants' tools always act on the active environment.
  - The guide's "The active environment" section says that scripts, areas, components and templates belong to it.
  - A tool call that was in progress when the environment changed must fail, not write into the new environment. The builder decides the mechanism, for example capturing the environment root when the call starts and refusing if it changed before the save.

## Assistants and the guide

- The guide explains that each environment has its own scripts and areas.
- The guide explains that saving writes only to the active environment.
- The guide explains how to ask the person to copy from another environment rather than rewrite.
- No new assistant tool for copying: copying is the person's action in the app. My call; it keeps the assistant from overwriting another environment's work.

## Playwright export (the owner's decision)

- **One export environment.** Settings names one environment as the Playwright export environment: the automation environment.
  - At the update it starts as the active environment.
  - The Playwright screen shows it and lets the person change it.
- **Export and Sync run only in that environment.** While any other environment is active, they are refused with one sentence. The sentence names the export environment and says to switch to it.
  - Pending and stale lists show nothing actionable in other environments and say why.
- **The export record lives only in the export environment's folder.** That is `projects/<slug>.pwexport.json`. The clone folder setting stays shared, since it is one repo.
- **Changing the export environment** asks for confirmation. The next Sync compares the new environment's scripts with what the repo holds, so cases that differ show as stale. Nothing is written until the person syncs.

## Backup and restore

My calls.

- A backup leaves out the update's legacy originals and the removed-environments area. It includes every environment's folder and `_seed`.
- **Importing a backup made before the update** (a flat `autorun/` with no `envs/`) replaces every current environment's per-environment data. It copies the backup's data into each environment, the same one-time copy the update does. Import already overwrites this machine's data, so this matches what import means. The confirmation dialog says so.
- Importing a backup made after the update restores the folders as they are.

## Sign-in recipe

My call. The per-project sign-in recipe (`projects/<slug>.json`) is per environment, because the sign-in page can differ between app versions. It is part of every copy.

When an environment has no recipe and no address of its own, it is refused as today, with `NO_SITE_ADDRESS`. That message also suggests copying from another environment.

## Copying one case

My call. "Copy to environment…" brings everything the script needs that the target lacks:

- the components it uses;
- its area, including variants;
- the saved setups it names;
- the sign-in recipe, if the target has none.

It never overwrites the target's existing area, component or setup. When the target has a different one with the same name, the confirmation lists it, and the copy uses the target's version.

If the script's account key doesn't exist in the target, the copy still happens. The confirmation says to add that account in the target's Accounts before running.

## Removed environments

My call. For 30 days, a removed environment's folder appears as a source in "Copy Auto Run data from…", labelled "removed", so the work can be brought into another environment. There is no separate restore screen.

## UI

- The Auto Run screen's header shows which environment's scripts are on screen. It already shows the active environment somewhere; make sure that is visible on Auto Run and API Templates.
- Copy actions as above.
- The How To Use guide gets a short section, with new screenshots where screens changed.

## Non-goals

- Merging or diffing scripts across environments.
- Syncing a fix from one environment to the others automatically.
- Per-environment test files.

## Anti-goals

- A script, area or template from one environment ever being read or written while another is active.
- Any loss of existing scripts at the update.
- An assistant's save landing in a different environment from the one it read.

## Constraints

- Rust owns all file I/O.
- No ADO DELETE.
- User-facing errors name no path.
- Bindings are regenerated, never hand-edited.
- All Rust tests go in `src-tauri/tests/suite/`.
- Caching follows the one-implementation rule.

## Left to the builder

- Exact folder names.
- The classification of each store and call site.
- How a store is told the active root: a global like today's `ROOT` updated on switch, or a root passed down.
- The in-progress-switch guard.
- UI wording and placement within the app's conventions.

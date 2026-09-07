# Retiring the Azure DevOps update feed — design

**Date:** 2026-09-07
**Status:** approved in conversation, awaiting written review
**Supersedes:** `2026-09-04-devops-update-feed-design.md`
**Scope:** the v2 (Tauri) app. Source moves to a company GitHub repo;
releases stay exactly where they are.

## Goal

The app went to Azure DevOps for updates first, falling back to the public
GitHub releases repo. That is being retired: updates come from GitHub
alone again. Separately, the source repository moves to a company-owned
private GitHub repo.

Two things this is **not**:

- The releases repo does **not** move. It stays
  `AvinAlwis/azure-devops-test-case-manager-v2-releases`, public, which is
  what every install in the field already reads. There is therefore no
  update migration to manage on the GitHub side at all.
- The `PHR-TCM` repository and its `releases` branch are **left exactly as
  they are**. Nothing in this work deletes, rewrites or force-pushes them.

## Decisions already made

| Decision | Choice |
| --- | --- |
| Releases repo | Unchanged: `AvinAlwis/azure-devops-test-case-manager-v2-releases`, public. |
| Source repo | Moves to the company's private GitHub repo. |
| `PHR-TCM` | Untouched. Its `releases` branch is left frozen, on purpose (see below). |
| Update sources after this | GitHub API, then the `latest/download` mirror — the two that predate the DevOps work. |
| `resolve` / `Attempt` | **Kept.** Pure, tested decision logic; it earns its place with one source or three. |
| Retention/pruning on DevOps | Deleted with the rest of the publishing path. |

## Update 2026-09-07: the hazard below does not apply

**Read access to `PHR-TCM` was never granted to anyone.** That was confirmed
after this design was written, and it removes the whole problem.

The freeze needs DevOps to *answer* — `check` stops at the first source that
returns `UpdateAvailable` or `UpToDate`. An install that cannot read the
repository gets a 401/403, which the source turns into `Attempt::Failed`,
and a failed attempt falls straight through to GitHub. Verified against the
shipped 1.23.1 code: `ado.rs` sets the no-access flag and returns `Err` on
either status, and `resolve` only short-circuits on a non-`Failed` attempt.

So no install in the field can be frozen, and **release N does not need to
go to DevOps at all**. It ships to GitHub like every release before 1.23.0,
the publishing path can be removed before or after it, and the sequencing
section below is history rather than instruction.

One residual, stated rather than hidden — and it is not the access case
above. It is the Settings switch "Only check Azure DevOps for updates"
(`tcm-v2-updates-github-off=1` in localStorage), left ON on a machine still
running 1.23.x:

- **Signed out** — which is every launch, since tokens are held in memory
  only — `check` takes its early return and reports `blocked: "Sign in to
  check for updates - GitHub updates are switched off in Settings."`
  before any source is asked.
- **Signed in** — `sources` returns only the DevOps source (the switch
  skips appending GitHub), and with no access that source fails; with
  nothing else to try, the result is `blocked`. GitHub is never reached
  either way.

That install never sees release N, in either state, and the hourly check
says nothing about it: it fails silently by design, not loudly. The
localStorage key lives in the WebView2 user-data directory, which a
Velopack update does not clear, so the setting is sticky across upgrades —
installing N does not turn it off.

There is also a second, independent reason no install can freeze on the
access question specifically: `get_fresh_token` returns an error when
there is no token to refresh, and tokens are held in memory only, so
**every launch-time check on 1.23.x is tokenless**. A tokenless check in
1.23.1 omits the DevOps source from `sources` entirely and consults only
GitHub — the access question never even arises on a fresh launch.

Operationally: before or alongside shipping release N, any machine still
on 1.23.x that ever turned that switch on must either turn it back off in
Settings (the switch still exists there, in that version) or be updated to
the new version directly rather than left to notice on its own.

The section that follows is kept because it explains why the code is shaped
the way it is, and why `PHR-TCM`'s `releases` branch is being left alone.

## The migration hazard (superseded — see above)

`check` stops at the first source that **answers**, and "you are up to
date" is an answer:

```rust
Attempt::UpToDate => return UpdateStatus { ... },
```

An install on 1.23.0 or 1.23.1 asks DevOps first. If we merely stopped
publishing there, `PHR-TCM` would keep answering "1.23.1 — up to date"
for ever, and those installs would **never reach GitHub again**. They
would freeze silently, with no error and no banner.

The fix does not require touching the branch. It requires the **last
thing ever published to DevOps to be the build that stops reading
DevOps**:

1. Publish the removal build (call it **N**) to **both** DevOps and
   GitHub. This is the final DevOps publish.
2. Every 1.23.x install asks DevOps, finds N, and updates to it. Every
   older install asks GitHub, finds N, and updates to it. One release
   migrates everybody.
3. N contains no DevOps source, so from then on it reads GitHub only.
4. The `releases` branch is then left frozen at N for good. That is not
   litter — it is what keeps migrating a straggler who does not open the
   app for months. Whenever they do, DevOps hands them N and they cross
   over.

This is why the code removal and the publishing removal cannot land in
the same release, and why the order below is not negotiable.

## Sequencing

**Release N — the app stops reading DevOps.**
Remove the app-side DevOps source and everything the user could see of
it. The release script keeps its DevOps publishing for this one release,
because N has to reach 1.23.x installs through DevOps.

**After N — the publishing path goes.**
Remove the DevOps half of the release script, `prune-releases.mjs` and
`phr-tcm-README.md`. No release is required for this; it takes effect on
whatever ships next. From here the script is: gates → push source →
build → pack → `vpk upload github`.

Getting this backwards — removing the publishing path first — leaves
every 1.23.x install permanently frozen, with the app reporting itself up
to date. That is the one failure mode this design exists to avoid.

## What is removed

### Rust

- `v2/src-tauri/src/updater/ado.rs` — deleted in full (`AdoSource`,
  `AccessDenied`, `ADO_ITEMS_BASE`, `ADO_BRANCH`, `TCM_UPDATE_BRANCH`).
- `v2/src-tauri/tests/updater_ado.rs` — deleted with it.
- `v2/src-tauri/src/updater/mod.rs` — `sources`, `check` and
  `download_and_apply` lose their `token` and `github_off` parameters;
  the `"ado"` entry and the `AccessDenied` plumbing go; `UpdateStatus`
  loses `no_access`. The early return for "GitHub off and no token" goes
  with the switch that motivated it.
- `v2/src-tauri/src/commands/misc.rs` — `check_update` and `apply_update`
  lose their `github_off` argument and stop fetching a token.
- `v2/src-tauri/Cargo.toml` — velopack's `public-utils` feature reverts;
  it was enabled only so `AdoSource` could name `bundle::Manifest` and
  reuse velopack's downloader.

`updater/mod.rs` stays a directory module (`updater/mod.rs`) rather than
moving back to `updater.rs`: the move is churn with no benefit, and the
path appears in the crate's own comments.

### Frontend

- `v2/src/lib/updatePrefs.ts` and `updatePrefs.test.ts` — deleted.
- `v2/src/screens/Settings.tsx` — the "Only check Azure DevOps for
  updates" switch and its helper text.
- `v2/src/App.tsx` — the "updates have moved" notice, `movedDismissed`
  and its per-episode reset, and the `githubOffSnapshot()` arguments.
  The `["update"]` invalidation on sign-in goes too: it existed because a
  launch check ran without a token and so skipped DevOps, and with GitHub
  alone a check needs no token at all.
- `v2/src/lib/updateToast.ts` — `UPDATES_MOVED` and both `no_access`
  branches, restoring the plain three-way report.
- `v2/src/components/CommandPalette.tsx`, `v2/src/dev/demo.ts` — the
  argument and the stub field.
- Tests in `App.test.tsx`, `Settings.test.tsx`, `updateToast.test.ts`
  that cover the notice, the switch and `no_access`.
- `v2/src/bindings.ts` — regenerated, not hand-edited.

### Publishing (after N)

- `v2/scripts/prune-releases.mjs` and `prune-releases.test.ts` — deleted.
  They existed only to maintain the DevOps branch.
- `v2/scripts/phr-tcm-README.md` — deleted.
- `v2/scripts/release-v2.ps1` — the clone/prune/orphan-commit/force-push
  block, `$RELEASE_BRANCH`, `-DevOpsBranch`, `-SkipGitHub`,
  `-SkipSourcePush` and their three guards. The both-or-neither failure
  message goes with them; with one destination there is nothing to keep
  in step.

## The source move

Almost not a code change. `release-v2.ps1` pushes with `git push origin
HEAD`, so repointing the remote is the whole of it:

```
git remote set-url origin <company repo URL>
```

What does need editing is documentation and any hardcoded URL:

- `README.md` and `CLAUDE.md` — the source repo, and the removal of the
  DevOps update description.
- The `v1` branch is unaffected and stays on the current remote unless it
  is pushed to the new one deliberately.

`$repoUrl` in the release script points at the **releases** repo, not the
source, and does not change.

## What the user sees afterwards

Nothing, on a good day. Updates arrive from GitHub as they did before
1.23.0. Specifically gone: the "App updates have moved to Azure DevOps"
notice, the Redmine-ticket wording, and the Settings switch.

`UpdateStatus` returns to three outcomes — a newer version, genuinely up
to date, and "no check happened" — which is the contract the file's own
header documents.

## Testing

- The Rust suite with `updater_ado.rs` gone: `sources()` yields exactly
  `["github api", "latest/download"]`, and `resolve` keeps its
  available / up-to-date / blocked / no-attempts behaviour.
- `cargo test --test bindings` regenerates `bindings.ts`; `no_access` and
  `githubOff` must be absent from it afterwards.
- `npx tsc --noEmit` is the real gate on the frontend removal: every
  caller of the two commands must stop passing an argument that no longer
  exists.
- The full frontend suite, with the notice/switch tests removed rather
  than left asserting behaviour that is gone.
- Manual, once N is installed: check for updates and confirm the result
  comes from GitHub, with no Settings switch and no notice.

## Out of scope

- Moving the releases repo. It stays public and where it is.
- Any change to `PHR-TCM`, including deleting the `releases` branch.
- The `v1` branch.
- Adding CI to the new source repo (worth doing; not this).

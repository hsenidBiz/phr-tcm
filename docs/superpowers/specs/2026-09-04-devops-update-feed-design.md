# Update feed on Azure DevOps — design

**Date:** 2026-09-04
**Status:** approved in conversation, awaiting written review
**Scope:** the v2 (Tauri) app only. The source repo stays on GitHub.

## Goal

The app checks a repository in the company's Azure DevOps for updates,
using the sign-in it already has, and falls back to the existing public
GitHub releases repo when DevOps cannot answer. A user who is signed in
and can read the DevOps repo gets updates from it; a user who cannot is
told that updates have moved and how to ask for access, while still being
updated from GitHub for as long as GitHub is published to.

Nothing in this design changes how an update is *applied*: Velopack does
that exactly as it does today. Only where the feed and the packages are
read from changes, plus the words shown when DevOps says "no".

## Decisions already made

| Decision | Choice |
| --- | --- |
| Source repo | Stays on GitHub (`AvinAlwis/azure-devops-test-case-creator`). |
| Releases repo on DevOps | `https://dev.azure.com/PeoplesHR/HRM/_git/PHR-TCM`, branch `releases`. NOT `main`: `main` carries a policy requiring a pull request, which rejected 1.23.0 with TF402455 - a release replaces the branch with one orphan commit, which no protected branch can accept. `main` keeps the DevOps template README. |
| Retention on DevOps | The newest **5** versions. |
| Fallback | GitHub releases repo, tried when DevOps fails for any reason. |
| Update check timing | Unchanged: at launch and hourly. Without a token the check skips DevOps and goes straight to GitHub. |
| No access to `PHR-TCM` | Show a notice asking for a Redmine ticket, even when GitHub still serves the update. |
| First install | `AzureDevOpsTestCaseManager.V2-win-Setup.exe` is downloaded from the repo's file view in DevOps (sign-in required) and run. |
| Proving DevOps alone works | A Settings switch, **Only check Azure DevOps for updates**, drops the GitHub sources entirely. A testing aid; off by default. |
| Releases | Nothing is published as part of this work. The first real release after it is a separate, explicit instruction. |

## Why a git repo and not Azure Artifacts or pipeline artifacts

DevOps has no equivalent of GitHub Releases. Of the places it can hold a
binary, only a git repo serves files over plain HTTPS with a bearer
token — which is what the app already holds. Azure Artifacts' Universal
Packages need the `az` CLI; its NuGet feeds reject Velopack's layout,
where the full and the delta package share one id and version; pipeline
artifacts expire on retention and have no notion of "latest".

## Layout of `PHR-TCM`

Flat, at the root of `releases`:

```
README.md                                        how to install; the Setup.exe link
releases.win.json                                Velopack's feed, merged across the kept versions
AzureDevOpsTestCaseManager.V2-<ver>-full.nupkg   one per kept version
AzureDevOpsTestCaseManager.V2-win-Setup.exe      the newest version only
AzureDevOpsTestCaseManager.V2-win-Portable.zip   the newest version only
RELEASES                                         the newest version only (a legacy file vpk writes)
```

These are exactly the files `vpk pack` produces today, minus its internal
`assets.win.json`. There are **no delta packages**: none have ever been
published (GitHub 1.22.1 holds only a `-full.nupkg`), and adding them is
a separate change. The template's `src/`, `docs/` and `.gitignore` are
removed.

The release branch is always **one commit deep**: every release replaces it with a
fresh orphan commit and force-pushes. GitHub Releases store assets
outside git history, so deleting a release frees its space; a git repo
keeps history forever unless it is rewritten, and rewriting is the only
way to hold the size at roughly `5 × 13 MB` of packages plus one
installer and one portable zip — about 100 MB.

## The app side

### `AdoSource` — a Velopack `UpdateSource` over the Items API

New, in `v2/src-tauri/src/updater.rs`. Implements
`velopack::sources::UpdateSource`, whose two methods are synchronous:

```rust
fn get_release_feed(&self, channel: &str, app: &Manifest, staged_user_id: &str)
    -> Result<VelopackAssetFeed, velopack::Error>;
fn download_release_entry(&self, asset: &VelopackAsset, local_file: &Path,
    progress_sender: Option<Sender<i16>>) -> Result<(), velopack::Error>;
```

Construction takes the bearer token as a `String`. Both `check_update`
and `apply_update` in `commands/misc.rs` already run their work under
`tauri::async_runtime::spawn_blocking`; they `await
state::get_fresh_token` **before** spawning and hand the token in.

Velopack keeps `bundle::Manifest` — which the trait's signature names —
behind its `public-utils` cargo feature, so that feature is turned on.
It also makes Velopack's own downloader public, so the package download
reuses `velopack::download::download_url_to_file_with_headers` with the
bearer header (whole-percent progress included, floored to 5 as today).
The two small JSON requests (branch tip, feed) go through
`reqwest::blocking`, already enabled, because their status code has to be
read exactly: 401 and 403 are the "no access" signal.

`TCM_UPDATE_BRANCH` in the environment overrides the branch read (`releases`
by default). It exists for one purpose: rehearsing a release on a
throwaway branch from an installed build without touching the release branch.

Requests, all against
`https://dev.azure.com/PeoplesHR/HRM/_apis/git/repositories/PHR-TCM/`
with `api-version=7.1` and `Authorization: Bearer <token>`:

1. `refs?filter=heads/releases` — the commit id `releases` points at now.
2. `items?path=/releases.win.json&download=true&versionDescriptor.versionType=commit&versionDescriptor.version=<id>`
   — the feed, pinned to that commit.
3. `items?path=/<asset.FileName>&download=true&versionDescriptor…=<id>`
   — each package, pinned to the **same** commit, streamed to
   `local_file` with whole-percent progress sent on `progress_sender`.

The pin is what closes the race the GitHub source documents at the top of
`updater.rs`: a release pushed between reading the feed and downloading
the package can no longer make the two disagree, because both are read
from one commit. The commit id is captured in `get_release_feed` and
kept on the source for `download_release_entry`; `download_and_apply`
already re-checks before downloading, so a stale id is at worst one
re-check old.

### Source order

`sources()` becomes, in the order tried:

1. `ado` — `AdoSource`, **only when a token was supplied**; skipped
   entirely otherwise, so an unsigned-in launch behaves exactly as today.
2. `github api` — unchanged.
3. `latest/download` — unchanged.

`check` and `try_source` keep their existing fall-through: the first
source that answers wins, later ones are only tried when an earlier one
errors.

### The "Only check Azure DevOps for updates" switch

With GitHub as a fallback, a DevOps failure is invisible — the update
still arrives. So Settings → Updates gets a switch, off by default,
that leaves the two GitHub sources out of the list altogether. The
frontend keeps it (`localStorage`, key `tcm-v2-updates-github-off`) and
passes it as a boolean argument on every `check_update` and
`apply_update` call; the Rust side holds no copy. With it on and no
token, the check reports `blocked: "Sign in to check for updates -
GitHub updates are switched off in Settings."` rather than the
not-an-install message an empty source list would otherwise produce.
Its helper text says plainly that it is for checking DevOps works and
should normally stay off. A version served by DevOps and then downloaded from GitHub (or
the reverse) is fine — the file names, sizes and hashes are the same
`vpk pack` output published to both.

### Telling "no access" apart from "unreachable"

`check` records, per source, whether the failure was an HTTP 401 or 403
from DevOps. `UpdateStatus` gains one field:

```rust
/// DevOps answered 401/403: this user cannot read PHR-TCM. Independent
/// of `available` — GitHub may still have served an update.
pub no_access: bool,
```

`available` and `blocked` keep their meaning. The three outcomes of the
DevOps attempt are:

| DevOps result | `no_access` | Then |
| --- | --- | --- |
| No token | `false` | GitHub, silently — as today. |
| Network error, 404, 5xx, bad JSON | `false` | GitHub, silently; logged via `applog::warn` as today. |
| 401 or 403 | `true` | GitHub, and the notice below. |

### The notice

Rendered by the frontend in the same slot as the existing update banner
in `App.tsx`, whenever `update.data.no_access` is true. Copy:

> **App updates have moved to Azure DevOps.** You don't currently have
> access to the PHR-TCM repository, so you won't receive the latest
> updates. Please raise a Redmine ticket asking for read access to
> `HRM / PHR-TCM`.

Behaviour:

- Dismissible with an X; the dismissal lasts for the session (React
  state, not storage) and the notice returns on the next launch.
- Disappears on its own the first time a check comes back with
  `no_access: false`, so nothing is needed once access is granted.
- When an update is *also* available (GitHub served it), the update
  banner takes the slot and the notice stacks under it — both are true.
- Settings → "Check for updates" shows the same text in its result line,
  so the notice is not only a launch-time thing.

Wording follows the tour's rule: it says what the user sees and what to
do, not what the app did.

## Publishing — `v2/scripts/release-v2.ps1`

The publish step publishes to **both** places, DevOps first:

1. `vpk pack` as today, into `v2/Releases/`. Its `releases.win.json`
   names **only the version just packed**.
2. Clone `PHR-TCM` shallowly into a temp directory (the release machine's
   own git credentials, as the GitHub push already relies on).
3. `node scripts/prune-releases.mjs --repo <clone> --pack v2/Releases
   --keep 5`: merge the clone's feed with the pack's (the pack wins a
   tie), keep the newest 5 versions by numeric semantic version, copy
   their packages plus the latest-only files in, delete every other file
   in the clone except `.git` and `README.md`, and write the merged feed.
   A feed that names fewer packages than exist is fine; one that names
   more is not, which is why the feed is written last from what was kept.
4. Copy `scripts/phr-tcm-README.md` in as `README.md`.
5. `git checkout --orphan`, commit everything as one commit
   `release <ver>`, `git push --force origin HEAD:<branch>`.
6. Then the existing `vpk upload github …`, unchanged.

Two switches exist only for rehearsals: `-DevOpsBranch <name>` (default
the release branch) and `-SkipGitHub`, which the script refuses when the branch is
the release branch — a real release goes to both places or to neither.

Both or neither: if step 6 fails after step 5 succeeded, the script
stops with a message naming exactly that — DevOps is ahead of GitHub —
and the one command to re-run, so the fallback is never left a version
behind without anyone knowing. There is no automatic rollback.

GitHub is retired, whenever that is decided, by deleting step 6.
Installs that predate this change read GitHub only and keep working
until then; the first version carrying `AdoSource` reaches them through
GitHub, and from then on they read DevOps first.

## Testing

Automated, in the Rust crate:

- `AdoSource` against a local mock HTTP server: the three-request
  sequence, the commit pin appearing on every URL, the feed parsed, an
  asset streamed with progress reaching 100, and 401/403 surfacing as the
  distinct no-access error while a 500 surfaces as an ordinary error.
- The retention prune on a fake directory of eight versions: five kept,
  the feed rewritten to match, one Setup.exe.
- `check` with a stubbed source list: no token → DevOps skipped; DevOps
  403 + GitHub update → `available` set **and** `no_access: true`;
  DevOps 403 + GitHub error → `blocked` set and `no_access: true`.

Automated, in the frontend:

- The notice renders on `no_access: true`, not on `false`; dismisses;
  and returns on a fresh mount.
- The update banner and the notice can both show at once.
- Settings' result line shows the no-access text.

Manual, before the first real release, from an installed build:

- Publish a throwaway version to a `PHR-TCM` **branch** (not the release branch) with
  the script pointed at that branch, confirm the installed app sees and
  downloads it, then delete the branch. the release branch is untouched until a real
  release is asked for.
- Sign out and confirm the check still reaches GitHub.
- With an account that cannot read `PHR-TCM`, confirm the notice appears
  and the update still installs.

## Out of scope

- Moving the source repo to DevOps.
- Any change to `packId` (`AzureDevOpsTestCaseManager.V2`) — existing
  installs update in place only because it does not change.
- Publishing a release. This design ships in the next release the user
  asks for, not as part of the implementation.

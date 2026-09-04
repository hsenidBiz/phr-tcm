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
| Releases repo on DevOps | `https://dev.azure.com/PeoplesHR/HRM/_git/PHR-TCM`, branch `main`. Exists today as an empty DevOps template. |
| Retention on DevOps | The newest **5** versions. |
| Fallback | GitHub releases repo, tried when DevOps fails for any reason. |
| Update check timing | Unchanged: at launch and hourly. Without a token the check skips DevOps and goes straight to GitHub. |
| No access to `PHR-TCM` | Show a notice asking for a Redmine ticket, even when GitHub still serves the update. |
| First install | `TestCaseManager-win-Setup.exe` is downloaded from the repo's file view in DevOps (sign-in required) and run. |
| Releases | Nothing is published as part of this work. The first real release after it is a separate, explicit instruction. |

## Why a git repo and not Azure Artifacts or pipeline artifacts

DevOps has no equivalent of GitHub Releases. Of the places it can hold a
binary, only a git repo serves files over plain HTTPS with a bearer
token — which is what the app already holds. Azure Artifacts' Universal
Packages need the `az` CLI; its NuGet feeds reject Velopack's layout,
where the full and the delta package share one id and version; pipeline
artifacts expire on retention and have no notion of "latest".

## Layout of `PHR-TCM`

Flat, at the root of `main`:

```
README.md                                       how to install; the Setup.exe link
releases.win.json                               Velopack's feed, as `vpk pack` writes it
AzureDevOpsTestCaseManager.V2-<ver>-full.nupkg  one per kept version
AzureDevOpsTestCaseManager.V2-<ver>-delta.nupkg one per kept version (except the oldest kept, which may have none)
TestCaseManager-win-Setup.exe                   the newest version only
```

The template's `src/`, `docs/` and `.gitignore` are removed.

`main` is always **one commit deep**: every release replaces it with a
fresh orphan commit and force-pushes. GitHub Releases store assets
outside git history, so deleting a release frees its space; a git repo
keeps history forever unless it is rewritten, and rewriting is the only
way to hold the size at roughly `5 × 30 MB + 28 MB`.

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
state::get_fresh_token` **before** spawning and hand the token in. HTTP
is `reqwest::blocking`, whose feature is already enabled in `Cargo.toml`.
Velopack's own `download` module is private, so it cannot be reused with
custom headers.

Requests, all against
`https://dev.azure.com/PeoplesHR/HRM/_apis/git/repositories/PHR-TCM/`
with `api-version=7.1` and `Authorization: Bearer <token>`:

1. `refs?filter=heads/main` — the commit id `main` points at now.
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
errors. A version served by DevOps and then downloaded from GitHub (or
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

1. Clone `PHR-TCM` shallowly into a temp directory (the release machine's
   own git credentials, as the GitHub push already relies on).
2. `vpk download http --url <raw-items-url-of-main>` is **not** used: it
   needs an unauthenticated URL. Instead the clone *is* the previous
   release set — copy its `*.nupkg` and `releases.win.json` into the
   `Releases/` staging dir before `pack.ps1` runs, so `vpk pack` builds a
   delta against the last version. `pack.ps1` currently wipes
   `Releases/`; it gains a `-Previous <dir>` switch that seeds it after
   the wipe.
3. `vpk pack` as today.
4. Copy the pack output over the clone, then prune: keep the `full` and
   `delta` packages of the newest 5 versions (by semantic version parsed
   from the file name), delete every other `*.nupkg`, keep exactly one
   `TestCaseManager-win-Setup.exe`, and rewrite `releases.win.json` to
   list only the kept assets (Velopack tolerates a feed that names fewer
   packages than exist, not more).
5. `git checkout --orphan`, commit everything as one commit
   `release <ver>`, `git push --force origin HEAD:main`.
6. Then the existing `vpk upload github …`, unchanged.

Both or neither: if step 6 fails after step 5 succeeded, the script
stops with a message naming exactly that — DevOps is ahead of GitHub —
so the fallback is never left a version behind without anyone knowing.
There is no automatic rollback; the fix is to re-run the GitHub upload.

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

- Publish a throwaway version to a `PHR-TCM` **branch** (not `main`) with
  the script pointed at that branch, confirm the installed app sees and
  downloads it, then delete the branch. `main` is untouched until a real
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

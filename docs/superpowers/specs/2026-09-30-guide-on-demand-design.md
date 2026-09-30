# How To Use on demand: a smaller app, the guide downloaded when wanted

Design, agreed with the owner on 2026-09-30. Changes how the help site from
`2026-09-26-help-site-design.md` reaches people.

## 1. Why

The How To Use guide is 31 MB - almost all of it screenshots, at two
resolutions, light and dark - and it is compiled into the app. The app's
package is 37 MB (the Setup.exe 45 MB), so the guide is about 85% of every
install and of every update, and releases carry no delta packages, so every
update downloads all of it again.

The guide leaves the app. It is published beside the app on each release
and downloaded only when someone asks for it, then kept across updates.

Success: a new release's package is about 6-8 MB; a person who never opens
the guide never downloads it; a person who has it keeps it through every
app update and is offered "Update Guide" only when the guide really
changed.

## 2. Owner decisions

1. The app - installer and updates - ships **without** the guide.
2. Settings' button reads **Download How to Use** until the guide has been
   downloaded on this machine; that button downloads it, then opens it.
3. Once downloaded, the guide is **kept across app updates**.
4. When the guide for the running app version differs from the one on
   disk, **Update Guide** is offered beside How To Use. The copy on disk
   keeps opening meanwhile. Updating is the person's choice, never
   automatic.
5. The guide downloaded is the one published with **the running app's own
   release**, so its screenshots match the screens in front of the person.

## 3. What a release publishes

Beside Velopack's packages on the phr-tcm release (stable and beta alike):

- `how-to-use.zip` - the built site (`src-tauri/help/`, unchanged by this
  design), `index.html` at the zip's root.
- `how-to-use.json`:
  ```json
  { "fingerprint": "<64 hex>", "sha256": "<64 hex>", "size": 32505856 }
  ```
  - `fingerprint`: SHA-256 over the site's CONTENT - every file's path
    relative to the site root (forward slashes), sorted, each followed by
    its bytes' own SHA-256. It depends only on the files, never on the zip
    or its timestamps, so a release whose guide did not change publishes
    the same fingerprint as the one before.
  - `sha256` and `size`: of `how-to-use.zip` itself, checked by the app
    before it unpacks anything.

`scripts/release-v2.ps1` builds both from `src-tauri/help/` after its
gates, and uploads them with `gh release upload` to the release Velopack
just published (`--clobber`, so a re-run replaces them). A failed upload is
fatal, as the Velopack upload is: a release without its guide cannot be
opened from the button. `-DryRun` prints what it would upload. The old
personal feed (`-AlsoLegacy`) does not get the guide: no build that reads
the guide reads that feed.

The fingerprint is computed by the same rule in two places - the release
script and the app (for a guide it adopts, §6) - and a Rust test pins the
rule against a fixed folder, so they cannot drift apart unnoticed. The
script computes it with PowerShell's `Get-FileHash`; the app with `sha2`.

## 4. On this machine

The app's local data folder gains:

```
help/
  <fingerprint>/          the unpacked site (index.html at its root)
  installed.json          { "fingerprint": "<64 hex>" | null, "at": "<stamp>" }
```

`installed.json` names the folder in use. `fingerprint: null` marks a guide
adopted from before this change (§6) whose fingerprint is unknown.

## 5. The app

A new module, `src-tauri/src/guide.rs`, replaces the embedding in
`help.rs` (the `include_dir!` of `src-tauri/help/` goes, and with it the
31 MB). Three commands, all ungated (the guide is for everyone):

- `guide_status() -> GuideStatus` - `{ state, size }`:
  - `NotDownloaded` - no usable guide on disk.
  - `Ready` - a guide on disk; this version's published fingerprint equals
    it, or could not be fetched (offline: never nag).
  - `UpdateAvailable` - a guide on disk whose fingerprint differs from this
    version's published one (an adopted guide, fingerprint unknown, counts
    as different).
  - `size`: the published zip's size, when known, so the button can say
    "Download How to Use (31 MB)".
  The published `how-to-use.json` is fetched from
  `https://github.com/hsenidBiz/phr-tcm/releases/download/v<app version>/how-to-use.json`
  (github.com itself, not the API - the updater's reason for its mirror
  holds here too), at most once per app run, with a 10 s timeout.
- `guide_download()` - downloads `how-to-use.zip` from the same release to
  a temp file, emitting `GuideProgress { received, total }` as it goes;
  refuses more than 200 MB; checks `size` and `sha256` against
  `how-to-use.json`; unpacks into `help/.incoming-<n>/`, requiring
  `index.html` at its root; renames it to `help/<fingerprint>/`; writes
  `installed.json`; then removes every other guide folder. Any failure
  removes the temp file and the incoming folder and leaves the guide on
  disk (if any) exactly as it was.
- `open_help()` (existing) - opens `help/<installed>/index.html`. With no
  guide on disk it answers the "not downloaded" sentence below.

Unpacking refuses, before writing anything, any entry whose path is
absolute, has a drive letter, contains a `..` component, or would resolve
outside the incoming folder; both `/` and `\` are read as separators (a zip
made by Windows PowerShell may use either).

**Development builds** (`debug_assertions`) do none of this: `guide_status`
answers `Ready` and `open_help` opens `src-tauri/help/index.html` straight
from the repository (its path fixed at compile time), so `npm run
docs:build` is all it takes to see a guide change, as today.

**User-facing sentences** name no URL or path (the app's rule):

- not downloaded (from `open_help`): "Download How to Use from Settings first."
- download failed: "Could not download How to Use. Check your connection and try again - Settings, Logs has the details."
- checksum or unpack refused: "The downloaded guide was damaged, so it was not kept. Try again."
- no guide published for this version (404 on the json): "How to Use is not available for this version."

The raw error always goes to the app log.

## 6. People who already have the guide

The app before this change wrote the embedded guide to
`help/<app version>/` whenever it was opened. On first run of the new app,
if `installed.json` is missing and such a folder with an `index.html`
exists, the newest one is adopted: `installed.json` is written with
`fingerprint: null`. The person keeps a working How To Use and is offered
Update Guide (its fingerprint is unknown). A folder without `index.html` is
ignored.

## 7. Settings

The How To Use button (in the card it sits in today) becomes:

| State | Controls |
|---|---|
| NotDownloaded | **Download How to Use (31 MB)** - downloads, shows a progress bar and "12 of 31 MB", then opens the guide |
| Ready | **How To Use** - opens it, as today |
| UpdateAvailable | **How To Use** and, beside it, **Update Guide** - downloads the new one with the same progress, then opens it |

While downloading, the button reads "Downloading..." and is disabled; a
failure is a toast with the sentence from §5. The status is asked for when
Settings opens. Sizes are shown in whole MB.

## 8. Changelog and help text

- Changelog (the release that ships this): "The app download is much
  smaller: How To Use is now downloaded the first time you open it from
  Settings, and kept through updates."
- The guide's Settings section (`docs-site/src/content/settings.ts`) says
  the guide is downloaded the first time and updated from there. The
  screenshot is unchanged: the capture build is a development build, whose
  button reads How To Use.

## 9. Testing

Rust (`src-tauri/tests/suite/guide.rs`, new, with its `mod` line; the old
help-embedding tests in their module go or are rewritten):

- The fingerprint rule over a fixed temp folder equals a pinned hex string
  (the release script's rule, §3), and is independent of file order and
  timestamps.
- State: no guide -> NotDownloaded; guide + equal fingerprint -> Ready;
  different -> UpdateAvailable; json unreachable -> Ready (and
  NotDownloaded stays NotDownloaded); adopted (null) -> UpdateAvailable.
- Download against a fake fetcher (a trait for the two GETs, as
  `db::Runner` is for sqlcmd): success replaces the folder and removes the
  old one; a size or checksum mismatch keeps nothing and leaves the old
  guide; a zip entry with `..`, an absolute path, a drive letter, or a
  backslash-escape is refused; a zip without `index.html` at its root is
  refused; more than 200 MB is refused.
- Adoption of an old `help/<version>/`, newest first; one without
  `index.html` ignored.
- The download URLs are built from the app version and point only at
  `github.com/hsenidBiz/phr-tcm/releases/download/`.
- `tests/bindings.rs` regenerated.

Frontend: the three button states, the progress text, the toast on failure,
the button disabled while downloading.

Release script: `-DryRun` prints the zip, the json and the upload command.

Acceptance: the next release's package is under 10 MB; a clean install's
Settings shows Download How to Use (with its size), which downloads and
opens the guide; the release after that, with an unchanged guide, shows no
Update Guide.

## 10. Not in this change

- Delta (partial) app updates from Velopack - separate, and worth doing
  next: it shrinks updates further.
- Shrinking the screenshots themselves.
- Downloading the guide automatically, or in the background.
- Hosting the guide online.

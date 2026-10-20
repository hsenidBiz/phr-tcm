# Bundle check - October 2026

Backlog item "Bundle optimization": measure what the frontend bundle and the
installed app hold, then change only what is worth it. Measured on
2026-10-20 against `feat/backlog-31-39` at `c1d6d29f` (app version
2.2.3-beta.15). Sizes from `npm run build` use Vite's units (1 kB = 1000
bytes); KiB/MiB are 1024-based.

## Verdict

- **Worth it, done (size):** the app's fonts shipped a `.woff` copy of every
  face next to the `.woff2` one. WebView2 only ever loads the `.woff2`, so the
  49 `.woff` files (696 KiB) were dead weight embedded in the exe. They are
  no longer emitted: the frontend embedded in the exe drops from about
  **2.61 MiB to 1.93 MiB** (-688 KiB, about -1.6% of the exe and about -3%
  of the full installer package).
- **Worth it, done (startup):** the JavaScript parsed before the first render
  drops from **688.7 kB to 586.3 kB** (-15%; gzip 227.2 kB -> 193.0 kB). The
  Auto Run run window and the markdown renderer + HTML sanitiser it pulled in
  (`marked`, `dompurify`) were in the startup bundle; they now load on first
  use.
- **Not worth it:** everything else measured below. The frontend was already
  split per screen (every screen, the command palette, the changelog, the
  sign-in screen and the PDF viewer load on demand), and the exe is 95%
  Rust code and data, not frontend. Nothing in the Rust start-up is heavy
  enough to defer.

## What the frontend holds (`npm run build`)

| | Before | After |
|---|---:|---:|
| `dist/` total | 6,012,867 B in 207 files | 5,300,242 B in 168 files |
| JS chunks | 90 files, 2,738 KiB | 100 files, 2,742 KiB |
| PDF.js worker (`pdf.worker.min.mjs`) | 1,236 KiB | 1,236 KiB |
| CSS | 310 KiB | 307 KiB |
| Fonts `.woff2` | 46 files, 705 KiB | 46 files, 705 KiB |
| Fonts `.woff` | 49 files, 696 KiB | **none** |
| `dist/vendor/` (bundled pages) | 16 files, 170 KiB | 16 files, 170 KiB |
| **Embedded in the exe** (brotli, as Tauri stores it - estimate below) | **2,669 KiB** | **1,981 KiB** |

Tauri's default `compression` feature stores each `dist/` file brotli-
compressed inside the exe (confirmed: file names appear in `v2.exe`, file
contents do not). The "embedded" row recompresses `dist/` with brotli at
quality 9 to estimate that; it is not a byte count from a release build.
JS compresses about 3.3x, so 2.7 MiB of JS costs 0.8 MiB in the exe; font
files are already compressed and cost their full size.

### What loads at startup

`index.html` loads the entry chunk and its one static import; everything
else is a dynamic `import()`.

| Startup chunk | Before raw / gzip | After raw / gzip |
|---|---:|---:|
| entry `index-*.js` | 389.32 / 124.64 kB | 363.79 / 116.40 kB |
| shared `theme-*.js` | 299.42 / 102.54 kB | 222.51 / 76.64 kB |
| **JS total** | **688.74 / 227.18 kB** | **586.30 / 193.04 kB** |
| `index-*.css` | 317.51 / 62.30 kB | 314.03 / 61.76 kB |

What the startup JS is made of (module sizes before minification, after):
`react-dom` 630 KiB, the generated `bindings.ts` 77 KiB, `@base-ui/react`
(via `xiod-ui`) 125 KiB, `tailwind-merge` 100 KiB, React Query 83 KiB,
`App.tsx` 37 KiB, the MCP tool catalogue `lib/mcpTools.ts` 24 KiB, the UI
tour 25 KiB. All of that is needed to draw the first screen or to answer
the AI bridge on launch.

The sign-in chunk (`SignIn-*.js`, 258.8 kB, gsap + motion) also starts
loading at launch - on purpose, see below.

The full per-chunk lists are in the appendix.

## What the installed app holds

Files from the 2.2.3-beta.15 release on this machine (built by
`scripts/release-v2.ps1`; nothing was rebuilt for this note):

| File | Size |
|---|---:|
| `src-tauri/target/release/v2.exe` | 45,293,568 B (43.20 MiB) |
| `Releases/...-2.2.3-beta.15-full.nupkg` | 21,366,756 B (20.38 MiB) |
| `Releases/...-2.2.3-beta.15-delta.nupkg` (what an update downloads) | 4,051,555 B (3.86 MiB) |
| `Releases/...-win-Setup.exe` | 28,882,404 B (27.54 MiB) |
| `Releases/...-win-Portable.zip` | 21,365,708 B (20.38 MiB) |
| `Releases/how-to-use.zip` (separate, downloaded on demand from Settings) | 27,100,997 B (25.85 MiB) |

The exe's PE sections: `.text` (code) 26.61 MiB, `.rdata` (read-only data,
which includes the embedded frontend) 15.15 MiB, `.pdata` 1.29 MiB, the rest
under 0.2 MiB. `tauri.conf.json` has no `bundle.resources`, and the help
site is not embedded (it is downloaded beside the app). Besides `dist/`
the exe embeds only `src-tauri/web/` (180 KiB of page scripts and styles,
via `include_str!`) and `autorun/builtin_recipe.json`.

So the frontend is about 6% of the exe before this change and about 4.5%
after. The other ~41 MiB is the Rust crate and its dependencies. The release
profile is already `opt-level = "s"`, fat LTO, one codegen unit, `strip`.
Expected effect of this change on the next release: exe about 42.5 MiB, full
package and Setup.exe about 0.67 MiB smaller each (font files do not
compress further in the package). Deltas are unaffected - the fonts never
change between releases.

## Startup: what runs before the first window paints

Timings from the app's own log lines (`set up`, `page loaded`, `first screen
drawn`, `ready`), 36 launches of installed builds 2.1.2-beta.1 to
2.2.3-beta.12, 13-18 October, this machine:

| Stage | Median | Range |
|---|---:|---:|
| `run()` start -> end of Rust `setup` | 529 ms | 354-891 ms |
| `run()` start -> page loaded | 746 ms | 489-3,175 ms |
| page start -> first React frame | 365 ms | 159-2,756 ms |
| page start -> ready (session known, loading screen gone) | 1,049 ms | 620-3,636 ms |

**Rust (`src-tauri/src/lib.rs`).** Tauri creates the main window and its
WebView2 inside `Builder::build`, *before* the `setup` closure runs, so most
of the 529 ms is WebView2 starting, not app code. The closure itself does
small, local work: the log, the Rust cache (`cache.json`, 56 KB here), four
small settings files, an Auto Run purge and orphan-download sweep of the
app's own folder (which must finish before a run can start - documented
there), then the tray icon and showing the window. The one disk sweep that
can wait (leftover browser profiles in `%TEMP%`) is already on its own
thread. The update check is not in `setup`: it is a React Query fetch after
the first render, then hourly. **Nothing to defer here.**

**Webview (`src/main.tsx`, `src/App.tsx`).** `main.tsx` does only cheap,
synchronous setup (theme, click log, link handler) before rendering.
Development-only modules are behind a compile-time gate and absent from the
release bundle. Every screen is `React.lazy`. The one eager import that did
not need to be there was the Auto Run run window (`screens/AutoRun/ReplayPane`)
and, through it, `components/CaseText` -> `lib/markdown` -> `marked` +
`dompurify` - fixed here.

The time from first frame to ready (median ~680 ms) is the session check:
"Stay signed in" signs in again over the network before the app can choose
between the sign-in screen and the work screen. That network round trip is
the long pole of every launch and is not a bundle question.

## What changed

1. **`vite.config.ts` - `.woff2` only.** A small PostCSS step drops the
   `.woff` source from each `@font-face` that also has a `.woff2` one. It
   runs as a `Once` hook so it runs before Vite's own url() step, which is
   what copies referenced files into `dist/` (an `AtRule` visitor ran too
   late: the CSS lost the reference but the files were still emitted).
   Behaviour is identical: Chromium takes the first source it supports, and
   it has supported `.woff2` since long before WebView2 existed.
2. **`src/App.tsx` - the run window loads on first use.** `ReplayPane` is now
   `React.lazy`, rendered by a small `RunWindowHost` only while the run
   store says its window is open (`ReplayPane` already rendered nothing
   otherwise). A lazy component starts loading when it is first rendered,
   so the host keeps it out of the tree until then; and it subscribes on
   its own, so a run's progress re-renders the host, not all of `App`. The
   first time a run's window opens, its code arrives a moment later (a local
   read from the exe).

Honest scope of (2): most screens show case text, so `marked` and `dompurify`
still load on a normal launch - right after the session check, with the
first screen, instead of before the first render. What it buys is a shorter
critical path to the first frame and to starting the session check, not
fewer bytes per session. On this machine that is likely in the low tens of
milliseconds; it was not measured live (no app run for this note).

## Looked at and not worth changing

| Candidate | Numbers | Why not |
|---|---|---|
| Load the sign-in screen only when signed out | `SignIn-*.js` 258.8 kB (gsap 383 KiB + motion 383 KiB unminified) + 46 kB of helpers | It loads behind the loading screen while the session check waits on the network (median ~680 ms of idle time). Deferring it saves almost nothing for a kept session and makes a signed-out launch wait for it afterwards. The current design is deliberate (comment at `loadSignIn` in `App.tsx`). |
| Drop the non-Latin font subsets | Cyrillic + Cyrillic-Ext 171 KiB, Greek + Greek-Ext 123 KiB, Vietnamese 34 KiB of the 705 KiB `.woff2` | Fetched only when text contains those characters, but a test case title in Cyrillic or Greek would then render in a fallback font. That changes what users see. |
| Drop the Latin-Extended subset | 213 KiB | Same reason: accented letters (é, ö, š) are common in names in work items. |
| PDF.js | `pdf-*.js` 436.6 kB + worker 1,265 kB raw (about 0.45 MiB in the exe) | Already loaded only by the Crystal Reports preview. Needed for that feature; one build serves everyone. |
| Changelog, command palette, PR panel, Astryx island, Settings | 185 / 111 / 138 / 126 / 80 kB | All already lazy; none is on the startup path. |
| Split the MCP tool catalogue out of startup | `lib/mcpTools.ts` 24 KiB unminified | `App` needs the disabled-tool list on launch to tell the AI bridge which tools are on, and that list is filtered by the catalogue. Splitting it is a refactor for a few kB. |
| Lazy UI tour | `src/tour` 25 KiB unminified | `App` uses the tour's data and state at startup; the component alone is ~8 KiB. Not worth the risk to the tour. |
| `@astryxdesign/theme-neutral` | imported nowhere (Grep) | Unused, but it never reaches the bundle (only imported code is bundled), so removing it changes no shipped byte. Left for a dependency tidy-up. |
| Rust dependencies | exe `.text` 26.6 MiB, `.rdata` 15.2 MiB | The large dependencies (`tiberius` via `rpt-db-mssql`, `pdf-extract`, `calamine`, `xcap`, `cpal`, `rustfft`, `tokio-tungstenite`, `reqwest` + native TLS, `image`, `notify`, Velopack) each back a shipped feature. Removing any changes behaviour. Attribution per crate needs `cargo bloat --release --crates`, which is a release build - not run here. |
| `panic = "abort"` | typically a few % of `.text` | Rejected on purpose: the crate relies on unwinding (see the comment on `[profile.release]` in `Cargo.toml`). |

## Left for later

- Run `cargo bloat --release --crates` (needs `cargo install cargo-bloat`
  and one release build) when there is time for one. If one crate dominates
  `.text` for a feature few people use, that is the only size lever left
  of any size.
- A log line at the very start of the `setup` closure would split the 529 ms
  "set up" figure into WebView2 creation vs. the app's own setup work. Cheap,
  but a Rust change, so not mixed into this one.
- After the next release: confirm `v2.exe` and the full package shrank by
  about 0.67 MiB, and compare the "first screen drawn" / "ready" medians in
  the log against the table above.

## Appendix: every chunk

`npm run build` output, largest first. Raw and gzip as Vite reports them
(no gzip figure for the PDF.js worker, which Vite copies as an asset).
Fonts are listed in the tables above, not here.

### Before

| Chunk | Raw kB | Gzip kB |
|---|---:|---:|
| `pdf.worker.min-Dswkl-cV.mjs` | 1265.41 | - |
| `pdf-CDa-wzR_.js` | 436.62 | 130.80 |
| `index-6JWHfh5S.js` | 389.32 | 124.64 |
| `index-DK8jzGIX.css` | 317.51 | 62.30 |
| `theme-BHLKtkbP.js` | 299.42 | 102.54 |
| `SignIn-BXgWDVuZ.js` | 258.80 | 93.88 |
| `changelog-CfsjIQVZ.js` | 185.01 | 59.42 |
| `index-BoUryFFu.js` | 178.83 | 49.06 |
| `PrPanel-DDNQhPew.js` | 137.63 | 43.52 |
| `AstryxIsland-BngecMzD.js` | 125.79 | 36.72 |
| `CommandPalette-QvUwtk2n.js` | 110.89 | 37.59 |
| `Settings-DAKEKDxy.js` | 79.86 | 28.67 |
| `WorkBoard-Btj3gqkk.js` | 73.91 | 23.33 |
| `index-CbhmQXN4.js` | 65.12 | 18.56 |
| `QueueSection-d7d5hOu5.js` | 56.19 | 18.19 |
| `index-DLCIs2Lh.js` | 46.39 | 14.22 |
| `Triangle-DfWMHKQ0.js` | 44.37 | 12.91 |
| `AiBridge-B_FpcmQB.js` | 40.70 | 11.35 |
| `RunTests-B2TeN1dN.js` | 25.78 | 8.84 |
| `EditCases-CIbmwHxa.js` | 24.72 | 7.71 |
| `RunnerWindow-Dsrktxsc.js` | 24.71 | 8.68 |
| `ImportFile-ChlgnTKv.js` | 22.25 | 7.96 |
| `index-LRcjCfYr.js` | 17.75 | 6.37 |
| `index-H2dDqW02.js` | 14.97 | 5.27 |
| `HistoryDots-FYYFkRC8.js` | 14.40 | 6.08 |
| `validate-CodDq7mp.js` | 13.79 | 5.16 |
| `window-DuOcSNx-.js` | 13.41 | 3.35 |
| `index-D-j-wvRy.js` | 10.67 | 4.38 |
| `Suites-B5OIGfft.js` | 10.58 | 3.90 |
| `CaseOrderList-BA6FnuCO.js` | 8.55 | 3.14 |
| `CreateWorkItem-p6TESE2B.js` | 6.10 | 2.14 |
| `combobox-CCYaig2O.js` | 5.04 | 2.08 |
| `ManualEntry-Dj6TGN-D.js` | 4.97 | 2.11 |
| `TestFilesDialog-C3EGiUel.js` | 4.95 | 1.99 |
| `webviewWindow-D90WpF3d.js` | 4.82 | 1.44 |
| `MoreActionsMenu-ChYMoEvH.js` | 4.20 | 1.74 |
| `multiselect-Xx5p9PEl.js` | 4.18 | 1.85 |
| `collapse-CJJpN-ak.js` | 3.40 | 1.47 |
| `history-CwALbZ_G.js` | 2.77 | 1.22 |
| `index-BUY4Bq8A.js` | 2.31 | 0.85 |
| (sign-in screen helper) | 2.02 | 1.12 |
| `PickPbiEmpty-D5-L_nHZ.js` | 2.01 | 0.86 |
| `grouping-D6xZ2_SQ.js` | 1.92 | 0.94 |
| `Tooltip-L-rbnOBF.js` | 1.78 | 0.80 |
| `InlineDiff-Cv5390Mz.js` | 1.71 | 0.77 |
| `ActionDock-y8wrQrum.js` | 1.46 | 0.82 |
| `ChangelogModal-5yoiY3eY.js` | 1.40 | 0.71 |
| `reportTheme-KfgZ24AE.js` | 1.19 | 0.56 |
| `CountTo-CgN76y06.js` | 1.03 | 0.61 |
| `suiteTree-C8oSvYZb.js` | 0.88 | 0.49 |
| `openRunner-BrThkQrV.js` | 0.85 | 0.48 |
| `ScanProgress-DSRG8ZOY.js` | 0.85 | 0.49 |
| `switch-CtnUSsw3.js` | 0.85 | 0.51 |
| `collapsedGroups-B0XgOuEQ.js` | 0.66 | 0.29 |
| `bug-BKYGP2Tj.js` | 0.63 | 0.33 |
| `exportDir-_9DZttfK.js` | 0.56 | 0.33 |
| `runnerSession-doD4nrvB.js` | 0.52 | 0.30 |
| `zoom-out-CdsRZmfi.js` | 0.50 | 0.24 |
| `suiteCasesQuery-DhbAR-Fk.js` | 0.48 | 0.33 |
| `video-CV8vfWcP.js` | 0.46 | 0.32 |
| `TagsField-CsRtzg21.js` | 0.46 | 0.31 |
| `eye-off-DIWu_Flj.js` | 0.43 | 0.27 |
| `clipboard-Dr0NltrV.js` | 0.43 | 0.33 |
| `ChangelogVersionTitle-wL1xMrqY.js` | 0.41 | 0.28 |
| `file-text-CkijSDlX.js` | 0.39 | 0.25 |
| `share-2-9xEaXiwY.js` | 0.36 | 0.24 |
| `list-ordered-D5a3Ug6P.js` | 0.36 | 0.23 |
| `pin-BJS3-7eZ.js` | 0.35 | 0.24 |
| `trash-2-Ch9PIRf_.js` | 0.33 | 0.22 |
| `badge-CunyH1KX.js` | 0.33 | 0.27 |
| `clipboard-check-vjjnqDrn.js` | 0.31 | 0.24 |
| `case-sensitive-Bk-4QZ9d.js` | 0.29 | 0.24 |
| `folder-open-DMrSnunw.js` | 0.29 | 0.22 |
| `pencil-p3SHcwYW.js` | 0.28 | 0.23 |
| `updateToast-CFlnXBxn.js` | 0.28 | 0.23 |
| `index-BL_XeLJM.js` | 0.28 | 0.18 |
| `eraser-DG837W7_.js` | 0.28 | 0.22 |
| `triangle-alert-BTPbG0pT.js` | 0.27 | 0.22 |
| `import-DnI5et5s.js` | 0.26 | 0.20 |
| `useIsFetching-zACq63nO.js` | 0.24 | 0.20 |
| `globe-BuDLZ5CH.js` | 0.24 | 0.19 |
| `database-BBbyV07x.js` | 0.24 | 0.20 |
| `folder-CPtUeqxf.js` | 0.23 | 0.19 |
| `download-XWQzBwba.js` | 0.23 | 0.19 |
| `paperclip-CyLMIokx.js` | 0.23 | 0.20 |
| `message-square-DMlgXE5a.js` | 0.23 | 0.20 |
| `undo-2-Dwp8bpWS.js` | 0.21 | 0.18 |
| `lock-open-BIHJ2nMi.js` | 0.21 | 0.20 |
| `skeleton-Di7HD7v_.js` | 0.20 | 0.18 |
| `play-DxzXqcxO.js` | 0.19 | 0.18 |
| `chevrons-down-up-54tWzWW-.js` | 0.18 | 0.16 |
| `plus-CB7H1rUL.js` | 0.15 | 0.15 |
| `SignIn-Cndoe-KK.css` | 0.09 | 0.10 |
| `Settings-CPevmvJn.css` | 0.07 | 0.09 |
| **94 files** | **4387.18** | **973.96** |

### After

| Chunk | Raw kB | Gzip kB |
|---|---:|---:|
| `pdf.worker.min-Dswkl-cV.mjs` | 1265.41 | - |
| `pdf-CDa-wzR_.js` | 436.62 | 130.80 |
| `index-DjftMMbM.js` | 363.79 | 116.40 |
| `index-T2eGTmrX.css` | 314.03 | 61.76 |
| `SignIn-CLjLykFr.js` | 258.80 | 93.88 |
| `theme-2hBFBHH4.js` | 222.51 | 76.64 |
| `changelog-D0seIBcd.js` | 185.01 | 59.42 |
| `index-D3jz5Iv2.js` | 179.11 | 49.16 |
| `PrPanel-CXGdbQex.js` | 137.74 | 43.57 |
| `AstryxIsland-Xtqq2vCU.js` | 125.79 | 36.72 |
| `CommandPalette-Tj4kvaHm.js` | 110.94 | 37.62 |
| `Settings-6iYFbUPc.js` | 79.90 | 28.69 |
| `markdown-9iA-cmBF.js` | 74.50 | 25.18 |
| `WorkBoard-CwD-xAzp.js` | 74.05 | 23.38 |
| `index-DY0EdIoh.js` | 65.30 | 18.63 |
| `QueueSection-DGMHaxj3.js` | 56.34 | 18.24 |
| `index-CTKW7ghW.js` | 46.54 | 14.28 |
| `Triangle-DfWMHKQ0.js` | 44.37 | 12.91 |
| `AiBridge-r60LtiWL.js` | 40.77 | 11.38 |
| `RunTests-DKMfhA-Q.js` | 25.99 | 8.93 |
| `EditCases-CFjgtmIi.js` | 24.94 | 7.79 |
| `RunnerWindow-CevEYbuq.js` | 24.80 | 8.72 |
| `ImportFile-B5eRVkFD.js` | 22.46 | 8.05 |
| `index-BgcsoyF-.js` | 17.78 | 6.38 |
| `index-DxaYe8u6.js` | 15.19 | 5.34 |
| `HistoryDots-ZUkpnxcf.js` | 14.43 | 6.09 |
| `validate-Dn7nZHgf.js` | 13.86 | 5.19 |
| `window-Tg5kDt9A.js` | 13.41 | 3.35 |
| `ReplayPane-CBsfsbmR.js` | 11.27 | 4.34 |
| `index-BBfGIPwq.js` | 10.72 | 4.41 |
| `Suites-B4-0azsR.js` | 10.65 | 3.94 |
| `checkbox-CeHAUbGE.js` | 8.67 | 3.62 |
| `CaseOrderList-B38zSU-o.js` | 8.56 | 3.14 |
| `CreateWorkItem-Ck1NBb9z.js` | 6.09 | 2.14 |
| `BatchList-Du2J9iQg.js` | 5.90 | 2.35 |
| `ManualEntry-DCWuBTGz.js` | 5.17 | 2.19 |
| `combobox-3yZ3lFHI.js` | 5.04 | 2.08 |
| `TestFilesDialog-C6EIUFZA.js` | 4.98 | 2.01 |
| `webviewWindow-BIy0Muoq.js` | 4.82 | 1.44 |
| `MoreActionsMenu-CuFzIZ6j.js` | 4.21 | 1.74 |
| `multiselect-D8Rk6tyr.js` | 4.21 | 1.87 |
| `collapse-DQeAX0BU.js` | 3.40 | 1.47 |
| `history-CwALbZ_G.js` | 2.77 | 1.22 |
| `index-Dk2UU8AU.js` | 2.31 | 0.85 |
| (sign-in screen helper) | 2.02 | 1.12 |
| `PickPbiEmpty-DW5JgE8h.js` | 2.01 | 0.86 |
| `grouping-D6xZ2_SQ.js` | 1.92 | 0.94 |
| `Tooltip-D_G01LdU.js` | 1.78 | 0.80 |
| `SharedStepLabel-DhGhS3fK.js` | 1.72 | 0.91 |
| `InlineDiff-CeEzIeBu.js` | 1.71 | 0.77 |
| `ActionDock-Cb-yQO3L.js` | 1.46 | 0.82 |
| `ChangelogModal-BBibRh2b.js` | 1.40 | 0.71 |
| `reportTheme-BulQ_gIU.js` | 1.19 | 0.56 |
| `CountTo-DqVI9k35.js` | 1.03 | 0.61 |
| `suiteTree-BBwgUrJu.js` | 0.88 | 0.49 |
| `openRunner-BENPT6T1.js` | 0.85 | 0.48 |
| `ScanProgress-CD63iiy3.js` | 0.85 | 0.49 |
| `switch-Co1hJi_z.js` | 0.85 | 0.51 |
| `collapsedGroups-KqDd6kAV.js` | 0.66 | 0.29 |
| `useTransitionStatus-E0I95uTD.js` | 0.64 | 0.33 |
| `bug-rU05uo7q.js` | 0.63 | 0.32 |
| `exportDir-_9DZttfK.js` | 0.56 | 0.33 |
| `runnerSession-doD4nrvB.js` | 0.52 | 0.30 |
| `zoom-out-BEpW3Wc1.js` | 0.50 | 0.23 |
| `suiteCasesQuery-G-137T-t.js` | 0.48 | 0.33 |
| `video-CTnHl8VX.js` | 0.46 | 0.32 |
| `TagsField-BQA_Mcx4.js` | 0.46 | 0.31 |
| `eye-off-CapG_gHC.js` | 0.43 | 0.27 |
| `clipboard-GwNROPnS.js` | 0.43 | 0.33 |
| `ChangelogVersionTitle-DfP5okVl.js` | 0.41 | 0.28 |
| `file-text-7Ucakh6z.js` | 0.39 | 0.25 |
| `share-2-C8Pakf9n.js` | 0.36 | 0.24 |
| `list-ordered-Cqeovtm9.js` | 0.36 | 0.23 |
| `useMediaQuery-B_9-BsQJ.js` | 0.36 | 0.24 |
| `pin-DfFTi-zQ.js` | 0.35 | 0.24 |
| `trash-2-CoOIEVY2.js` | 0.33 | 0.22 |
| `badge-BIbBggNZ.js` | 0.33 | 0.27 |
| `clipboard-check-YL12l80-.js` | 0.31 | 0.23 |
| `case-sensitive-CYBC1Hqm.js` | 0.29 | 0.24 |
| `folder-open-DFZFnttC.js` | 0.29 | 0.22 |
| `pencil-B8nLSx5B.js` | 0.28 | 0.23 |
| `updateToast-B-XyHPru.js` | 0.28 | 0.23 |
| `index-BHwbDAFU.js` | 0.28 | 0.18 |
| `eraser-BO9FVlJU.js` | 0.28 | 0.22 |
| `triangle-alert-190hWq-E.js` | 0.27 | 0.22 |
| `eye-BcjEIu3f.js` | 0.26 | 0.20 |
| `import-DP7tJs99.js` | 0.26 | 0.19 |
| `useIsFetching-Cibegp9m.js` | 0.24 | 0.20 |
| `globe-DFQm_Rll.js` | 0.24 | 0.19 |
| `database-HDAk1Btn.js` | 0.24 | 0.20 |
| `folder-CHvBtlHI.js` | 0.23 | 0.19 |
| `download-Bq9mqJ57.js` | 0.23 | 0.19 |
| `paperclip-C9qbbmC0.js` | 0.23 | 0.20 |
| `message-square-DRcJw-TC.js` | 0.23 | 0.20 |
| `undo-2-DxDuZnxu.js` | 0.21 | 0.18 |
| `lock-open-CmWo9rpl.js` | 0.21 | 0.20 |
| `skeleton-BJjsX_Q2.js` | 0.20 | 0.18 |
| `play-D5GhuRr3.js` | 0.19 | 0.18 |
| `chevrons-down-up-CHO3ijTa.js` | 0.18 | 0.16 |
| `plus-DEnr-r2x.js` | 0.15 | 0.15 |
| `chevron-down-DKOU9f0_.js` | 0.13 | 0.14 |
| `modalWidths-CIDt926W.js` | 0.10 | 0.11 |
| `SignIn-Cndoe-KK.css` | 0.09 | 0.10 |
| `Settings-CPevmvJn.css` | 0.07 | 0.09 |
| **104 files** | **4387.45** | **977.73** |

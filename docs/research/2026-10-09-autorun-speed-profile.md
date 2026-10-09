# Auto Run speed vs PHR-PLAYWRIGHT-AUTOMATION (read-only audit, 2026-10-09)

Paths are under `src-tauri/src/` unless they say otherwise.

## 1. Measured data: none available on this machine
- `%APPDATA%\com.avinalwis.testcasemanager.v2\autorun\` has only `scripts/` and `environments.json`. There is **no `runs/` folder and no `shots/`**. The active environment has a blank start address, so no unattended run has ever happened here.
- App logs from 2026-10-02 to 10-09: no Auto Run case, sign-in or lease lines, only UI navigation clicks.
- PHR: no `reports/` folder on disk.
- Even where runs exist, `CaseRecord.duration_ms` starts inside `run_case_in` (`autorun/replay.rs:340`). That leaves out the browser open and close (`one_go`, `replay.rs:689-739`), and steps carry no timestamps. A per-phase breakdown therefore cannot be read back from any run record.
- What could be measured is the script mix: **52 saved scripts, 4.5 steps and 18.9 actions per case on average (max 39)**. By kind: wait_for 486, check_text 189, click 175, check_url 59, navigate 57, fill 19. None set an account or `no_save`.
- PHR's own live measurements (`src/users/auth.ts` comments): the shell appears 3.4-5.2 s after the login click, and the active-session modal 187-288 ms after the shell.

The estimates below come from the code's constants. They are **not measured**.

## 2. Where the time goes (code audit)
**Per case**
- **A new browser and profile for every case.** `RealBrowsers::open` → `open_real` (`commands/autorun_replay.rs:76-89, 115-124`) launches Edge with a new `--user-data-dir` (`browser/launch.rs:170-172`). It always sleeps 250 ms before the first connect, then polls every 250 ms. Close does kill + wait + `remove_dir_all` synchronously (`commands/autorun.rs:533-539`).
- **Sign-in on every case**, but the saved session is reused (`autorun/signin.rs:217-248`, `session_minutes: 480`).
- **The optional-prompt windows run on every case, in sequence**, including the saved-session path (`signin.rs:237-239` runs `after_sign_in`). Built-in recipe (`autorun/builtin_recipe.json`):
  - sign-in steps: cookie 3000 ms (l.3), "Continue here" 3000 ms (l.9);
  - after_sign_in: cookie 4000 ms (l.13), bootbox 4000 ms (l.15), sidebar 5000 ms (l.17).

  A `when_visible` whose element never comes waits its **whole** window (`browser/actions.rs:1848-1859`, floor 500 ms at l.1841). `load_home` runs after_sign_in again on every trip home and on the module-trip retry (`autorun/nav.rs:526-556, 716`).
- **Module trip:** clicks the recorded menu path every case (`nav.rs:605-661`). PHR does the same: its product forbids deep links (`src/fixtures/test.ts:79-90`), so this is not a gap.
- **Transient retry:** reruns the whole case in a new browser, and setup first (`replay.rs:985-1008`). Setup retry pauses are 1/3/5 s (`api_templates/runner.rs:974`).

**Per step and per action**
- **A screenshot after every step**, passed or not (`replay.rs:506`), plus one on every failure (`runner.rs:568`). Each save prunes, which **reads and parses every run JSON on disk** (`autorun/store.rs:673-733` → `shots_of_unpublished_runs` → `list_runs` l.421-433). This is synchronous and grows with run history.
- **Click and fill need a second look 100 ms apart** before they act (`browser/input.rs:291-298, 372`): at least 100 ms each.
- **Highlight pause of 350 ms before every click and fill when watching** (`browser/timing.rs:35`). It is dropped only in unwatched runs (`commands/autorun_replay.rs:57-59`).
- **Waits poll every 100 ms** (`timing.rs:34`) and do not react to page events: wait_for `actions.rs:1505`, expect `expect.rs:279`, wait_ready `input.rs:372`, the arrival check `nav.rs:659`. Each found element is seen on average about 50 ms late, and each poll re-resolves the locator. Role locators call `Accessibility.queryAXTree` on each look (`browser/locator.rs:355`).
- **Default timeouts** (`timing.rs:31-36`): action 15 s, expect 10 s, nav 30 s, lease 60 s (`autorun/lease.rs:40`, poll 250 ms l.43). These match PHR's 15/10/30 s.
- **No snapshot calls** in the run path (`snapshot.rs` is used only by discovery). Page log and network record are in-memory event bookkeeping and cheap.
- **`no_save` scripts** pause every request (`urlPattern "*"`, `browser/save_guard.rs`) until Rust answers it (`cdp.rs:1171-1225, 1973`). That adds a per-request round trip. No current script uses it.

**Parallelism:** strictly one case at a time (the `for` loop at `replay.rs:889`) and one run at a time (`RUNNING`, `commands/autorun_replay.rs:22`).

## 3. PHR-PLAYWRIGHT-AUTOMATION (`playwright.config.ts`)
- **Workers:** `workers: CI ? 2 : 4` with `fullyParallel: false`, so up to 4 spec files run in parallel and tests inside one file run one after another. One browser per worker, a new context per test.
- **Session reuse:** `storageState` is minted once per account per 480 min (`src/users/auth.ts:310-324`). On reuse the only wait is `settleAfterColdStart`, a race between the login field and the shell marker that returns as soon as either shows (l.223-227). It runs **no optional-prompt windows**.
- **Optional prompts:** the cookie banner is a 0-wait sample (`isVisible`, l.252). The 5 s active-session window is paid only on a fresh login, and usually ends after about 250 ms.
- **Timeouts:** `actionTimeout` 15 s, expect 10 s, navigation 30 s, test 90 s.
- **No fixed sleeps anywhere** in `src/` or `suites/`: auto-wait is event-driven, and actionability waits about one animation frame.
- **Evidence:** screenshot and trace on failure only.

## 4. Ranked gaps (estimated saving per case)
| # | Gap | Est. saving | Change | Risk |
|---|---|---|---|---|
| 1 | Sequential optional-prompt windows on every sign-in | **7-13 s** (saved session: cookie 4 + bootbox 4 (+ sidebar 5 if already open); fresh login: Continue here 3 + cookie 4) | Skip the active-session wait on the saved-session path (no new login, so no modal). Race all optional prompts in **one** shared window (for example 1.5 s after the shell) instead of one after another. Make the cookie check a 0-wait sample, as PHR does. | Quirk 14: the modal arrives 187-288 ms late, so keep a short window on the fresh-login path. Quirk 2: wait for the shell first. If a modal is missed, every later click is swallowed; the transient retry covers it at full cost. The save guard is untouched: the sign-in is already held (`signin.rs:184,194`). |
| 2 | Parallel runs | Suite wall time ÷ up to N (PHR uses 4) | Run N cases at once on distinct accounts. The lease already serialises same-account cases. | Quirk 9: a second login kills the first session. Reset points, setup fixtures (one template run at a time) and shared drafts need ordering. The machine is shared (CPU). Quirk 13: the sandbox slows under load. |
| 3 | A new browser process and profile per case | ~1-3 s (launch, 250 ms first sleep, profile delete; more if antivirus scans each new profile) | One browser per run, a new `Target.createBrowserContext` per case (the Playwright model). Downloads per context. | Same cookie isolation as Playwright. One browser crash takes out the rest of the run, so relaunch on crash. The guard and tab tracking must cover targets in the new context. |
| 4 | Screenshot after every step + prune that parses every run file | ~0.3-1 s now, growing with history | Prune once per run, or cache the protected set. Keep the per-step evidence but take it off the critical path. | Low. Per-step pictures are review evidence, so keep them. |
| 5 | 100 ms polling | ~0.6 s (about 13 waits per case × 50 ms average) | Poll every 25-50 ms, or an in-page MutationObserver promise (`awaitPromise`) | Low. More CDP traffic. |
| 6 | Two-look stability check on click/fill (+350 ms highlight when watched) | ~0.4 s unwatched (about 4 per case + menu clicks); +1.3 s or more watched | Compare rects across two `requestAnimationFrame`s inside one `callFunctionOn` | Quirk 16 (the flyout animation race): rAF stability is what Playwright uses. |
| 7 | Role locators via the AX tree on every poll | Unmeasured | Resolve roles in-page with JS | Accessible-name fidelity |

**Biggest single win per case: #1.** The built-in recipe alone can spend 8-13 s per case sitting through prompt windows for prompts that never come. Projects with a recorded recipe may differ: check its `when_visible` windows.

**Before changing anything:** add per-phase timing (open, sign-in, module trip, each step) to the run record. Today's records cannot confirm any of these estimates.

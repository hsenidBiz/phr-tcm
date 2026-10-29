# Halloween Theme Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a seventh theme, Halloween (orange and black), with animated decorations. Every install switches to it once for 7 days, then goes back to what it had. Settings can change it at any time.

**Architecture:** Halloween is a new `ThemeId` in `src/lib/theme.ts` and a `:root[data-theme="halloween"]` palette in `src/index.css`. A new `src/lib/season.ts` owns the switch: a one-time record kept in localStorage, the 7-day restore, and cancelling the restore when the person picks something. A new `src/components/HalloweenDecor.tsx` draws pumpkins hanging under the PBI bar, bats, a cobweb and a spider. It renders only while `data-theme="halloween"` is set and never takes a click.

**Tech Stack:** React 19, TypeScript, Tailwind tokens, plain CSS keyframes in `src/index.css`, vitest/jsdom.

**Spec:** `docs/superpowers/specs/2026-10-29-halloween-theme-design.md`

## Global Constraints

- Never hardcode a colour in TSX. SVG fills and strokes use `var(--color-…)` or CSS classes. `src/ui-consistency.test.ts` is a gate: never weaken it.
- Every animation stops under `@media (prefers-reduced-motion: reduce)`.
- Decorations use `pointer-events: none` and `aria-hidden="true"`, and must never cover a control.
- The cut-off for switching people over is local time `2026-11-01T00:00`. Launches on or after it never switch.
- The restore comes `7 * 24 * 60 * 60 * 1000` ms after `switchedAt`.
- Run commands from the repo root: `npm test`, `npx tsc --noEmit`. Run one suite at a time.
- Commits use a Bash heredoc: `git commit -F - <<'EOF' … EOF`, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- The words "Halloween" and "pumpkin" are fine anywhere. This is not a hidden feature.

## Review Focus

1. **Someone who switched theme during the week must never be forced back.** Any `setThemeChoice` or `setAccent` call cancels the restore, including the sun/moon toggle and re-picking Halloween. Covered by test T1.d.
2. **Someone who had never picked anything.** Their keys are null, so the restore removes the keys rather than writing `"null"`. They land back on Light with violet. Covered by test T1.e.
3. **The app stays open past day 7 without a relaunch.** The hourly re-check in App applies the restore. Covered by test T1.f, which calls `initTheme()` again with a later clock.
4. **The pumpkins over the PBI bar's right-hand controls** (bell, settings, Work Manager pill) must not block clicks. They get `pointer-events: none` on every decoration node; T2.b checks the root. jsdom cannot hit-test, so this needs a manual check in the real window.
5. **The light/dark toggle after the week.** The switch must not write `tcm-v2-theme-dark`, so the moon toggle keeps going back to the person's own dark theme. Covered by test T1.g.

---

### Task 1: Halloween palette and the seasonal switch

**Files:**
- Modify: `src/lib/theme.ts`
  - Add `"halloween"` to `ThemeId`.
  - Add a `THEMES` entry `{ id: "halloween", label: "Halloween", dark: true, preview: { bg: "#0b0709", surface: "#17100f", accent: "#f97316" } }`. The preview hex values are allowed, like the other entries.
  - `initTheme` calls `runSeason()` first.
  - `setThemeChoice` and `setAccent` call `cancelSeasonRestore()`.
- Create: `src/lib/season.ts`
- Modify: `src/index.css`
  - Add `:root[data-theme="halloween"]` after the `oled` block, with every token the graphite block defines.
  - Values: bg `#0b0709`, surface `#17100f`, surface-2 `#221714`, border `#3a2620`, border-strong `#5a3a2c`, text `#fbefe3`, muted `#c4a99a`, faint `#8a7064`, accent `#f97316`, accent-hover `#fb923c`, accent-soft `rgb(249 115 22 / 0.16)`, on-accent `#0b0709`, danger `#f87171`, success `#4ade80`, warning `#fbbf24`, info `#60a5fa`.
  - If any theme block also defines tokens graphite lacks (such as `--color-accent-text`), define them here too.
- Modify: `src/App.tsx:424`. Replace `useEffect(() => initTheme(), [])` with an effect that calls `initTheme()` and then again every `60 * 60 * 1000` ms, clearing the interval on unmount.
- Modify: `src/accent-split.test.ts:46`. Add `Halloween: ':root[data-theme="halloween"]'` to `DARK_THEMES`, so the violet accent's contrast is checked on it too.
- Modify: `docs-site/src/content/settings.ts:75`. The sentence lists the themes; add Halloween: `"…Graphite, Ocean, Onyx or Halloween."`
- Test: `src/lib/season.test.ts` (new) and `src/lib/theme.test.ts`.

**Interfaces:**
- Produces, in `src/lib/season.ts`:
  - `SEASON_KEY = "tcm-v2-season-halloween-2026"`
  - `type SeasonRecord = { switchedAt: number; prevTheme: string | null; prevAccent: string | null; state: "active" | "done" }`
  - `runSeason(now: number = Date.now()): void`
    - With no record and `now < new Date(2026, 10, 1).getTime()`, it stores the raw localStorage values of `tcm-v2-theme-id` and `tcm-v2-accent` as prev. It writes `tcm-v2-theme-id = "halloween"` and `tcm-v2-accent = "default"` straight to storage, without calling `setThemeChoice` or `setAccent`, and records `state: "active"`.
    - With an active record and `now - switchedAt >= WEEK`, it restores each prev value (`removeItem` when it is null) and sets `state: "done"`.
    - Otherwise it does nothing.
  - `cancelSeasonRestore(): void`. An active record becomes `"done"`; with no record or a done one, it does nothing.
- Theme code applies the theme after `runSeason`: `initTheme` keeps calling `applyThemeChoice(getThemeChoice())` and `applyAccent(getAccent())`.
- localStorage access is wrapped in try/catch, as elsewhere in the app. A broken record (bad JSON) is treated as `done`, which never switches anyone.

- [ ] **Step 1: Write the failing tests** in `src/lib/season.test.ts`. Clear localStorage after each test. `OCT29 = new Date(2026, 9, 29, 12).getTime()`, `DAY = 86_400_000`.
  - T1.a: someone with `tcm-v2-theme-id = "ocean"` and `tcm-v2-accent = "blue"` runs `initTheme()` at OCT29. Then `getThemeChoice()` is `"halloween"`, `getAccent()` is `"default"`, `data-theme` is `"halloween"`, and the `dark` class is set. `initTheme` must accept the clock: give it `initTheme(now = Date.now())` and pass it through to `runSeason`.
  - T1.b: `initTheme(new Date(2026, 10, 1).getTime())` with no record switches nothing, and the theme stays `"ocean"`.
  - T1.c: switched at OCT29, `initTheme(OCT29 + 7 * DAY)` gives back theme `"ocean"` and accent `"blue"`. A further `initTheme(OCT29 + 8 * DAY)` stays on ocean, and `initTheme(OCT29 + 6 * DAY)` on a fresh switch stays on halloween.
  - T1.d: switched at OCT29, then `setThemeChoice("midnight")`; `initTheme(OCT29 + 8 * DAY)` keeps midnight. Repeat with `setThemeChoice("halloween")`, which keeps halloween. Repeat with `setAccent("rose")`, which keeps halloween and rose.
  - T1.e: with nothing stored, switch at OCT29 and restore at +7 days. `localStorage.getItem("tcm-v2-theme-id")` and `localStorage.getItem("tcm-v2-accent")` are both `null`, `getThemeChoice()` is `"light"` and `getAccent()` is `"violet"`.
  - T1.f: with a record already active, a second `initTheme(OCT29 + DAY)` changes nothing, and it never re-switches after a restore. After a restore and a later pick of `"slate"`, `initTheme(OCT29 + 2 * DAY)` keeps slate.
  - T1.g: with `tcm-v2-theme-dark = "ocean"`, the switch at OCT29 leaves `tcm-v2-theme-dark` as `"ocean"`.
  - T1.h: with `localStorage[SEASON_KEY] = "{not json"`, `initTheme(OCT29)` does not switch.
  - In `theme.test.ts`: `setThemeChoice("halloween")` sets `data-theme="halloween"` and the `dark` class.
- [ ] **Step 2:** `npx vitest run src/lib/season.test.ts src/lib/theme.test.ts` fails, because `season.ts` is missing.
- [ ] **Step 3:** Implement `season.ts`, then the theme.ts, index.css, App.tsx, accent-split and docs-site edits above.
- [ ] **Step 4:** `npx vitest run src/lib src/accent-split.test.ts src/ui-consistency.test.ts docs-site` passes, and `npx tsc --noEmit` is clean. If the docs guard says the built help page is stale, run `npm run docs:build` and commit `src-tauri/help/` with this task.
- [ ] **Step 5:** Commit: `feat(v2): a Halloween theme everyone is switched onto for a week`.

### Task 2: The decorations

**Files:**
- Create: `src/components/HalloweenDecor.tsx`
- Modify: `src/App.tsx`
  - Mount `<HalloweenDecor />` once, inside the signed-in column at `App.tsx:1383`, right after `<ContextBar …/>`.
  - The column wrapper gets `relative` if it lacks it. The decor positions itself `absolute` and never changes layout: it takes no height.
- Modify: `src/index.css`. Add a `/* Halloween decorations */` section of `.halloween-*` classes and keyframes, plus their entries in a reduce-motion block.
- Test: `src/components/HalloweenDecor.test.tsx`.

**Interfaces:**
- Consumes: the `data-theme="halloween"` attribute on `<html>` from Task 1.
- Produces: `export default function HalloweenDecor(): JSX.Element | null`.
  - It watches `document.documentElement` with a `MutationObserver` on `attributeFilter: ["data-theme"]` and renders `null` unless the attribute is `"halloween"`.

**The design (exact):**
- **Root.** `<div className="halloween-decor" aria-hidden="true">`, with CSS `position:absolute; inset:0 0 auto 0; height:0; overflow:visible; pointer-events:none; z-index:` just above the page content but below modals, toasts and popovers. Read the z-index scale in index.css and pick the value under the lowest overlay layer.
- **Pumpkins.** Four `.halloween-pumpkin` elements anchored to the top edge (just under the bar).
  - They sit at `right` offsets of about 22%, 31%, 40% and 48% of the width, so they land in the empty middle and stay off the right-hand controls and the PBI search on the left.
  - Each has a vertical `.halloween-string` of a different length (28, 44, 20 and 36 px) and an SVG jack-o'-lantern about 26 px wide:
    - body fill `var(--color-accent)`, ribs stroked with `var(--color-accent-hover)`;
    - stem `var(--color-success)`;
    - eyes and mouth `var(--color-warning)`, with a `.halloween-glow` flicker animation.
  - Sway: `transform-origin: top center; animation: halloween-sway 3.2s ease-in-out infinite alternate`, rotating between -6deg and 6deg, with each pumpkin given its own `animation-delay`.
- **Bats.** Three `.halloween-bat` SVGs (dark silhouettes, `fill: var(--color-border-strong)`, about 18 px), placed `position: fixed` near the top of the window.
  - Each crosses the window left to right with `halloween-fly` over 14 to 22 s, drifting a little in y, and stays off screen for most of its cycle. Give it its own delay, so a bat is in view roughly a third of the time.
  - Wings flap with `halloween-flap` (scaleY 1 → 0.4) at 0.25 s.
- **Cobweb.** One SVG web, `position: fixed`, in the bottom-left corner, stroked with `var(--color-border-strong)` at opacity 0.6, about 90 px.
- **Spider.** One spider drops from the bar's top-left area (about 12% from the left) on a thread every 20 s. It goes down about 60 px and back up (`halloween-drop`), body `var(--color-text)` at 0.8.
- **Reduced motion.** Under `prefers-reduced-motion: reduce`, every `.halloween-*` animation is `animation: none`. The bats are hidden (`display:none`), since a frozen bat stuck mid-screen looks like a bug. The spider stays at its resting height.

- [ ] **Step 1: Write the failing tests** in `HalloweenDecor.test.tsx`.
  - T2.a: renders nothing (an empty container) while `data-theme` is unset or `"graphite"`. After `document.documentElement.setAttribute("data-theme", "halloween")` and `await` (findBy), it shows `.halloween-decor` with 4 `.halloween-pumpkin`, 3 `.halloween-bat`, 1 `.halloween-web` and 1 `.halloween-spider`. Setting `data-theme` back to `"ocean"` removes it.
  - T2.b: the root has `aria-hidden="true"`. In the CSS text read from `src/index.css`, the `.halloween-decor` rule contains `pointer-events: none` and the file has a reduce-motion block that names `.halloween-pumpkin`, `.halloween-bat` and `.halloween-spider`. Read the file as other tests do: see `AssistantScene.test.tsx`'s CSS checks.
  - T2.c: no `#` hex and no `rgb(` appear in `HalloweenDecor.tsx`'s source. The ui-consistency gate also covers this; this test pins it locally.
- [ ] **Step 2:** `npx vitest run src/components/HalloweenDecor.test.tsx` fails.
- [ ] **Step 3:** Implement the component, the CSS and the mount.
- [ ] **Step 4:** `npx vitest run src/components/HalloweenDecor.test.tsx src/ui-consistency.test.ts src/collapse-motion.test.ts`, then the whole `npm test`. It must be all green, apart from the documented `App.test.tsx` flake, which must pass on a re-run. `npx tsc --noEmit` is clean.
- [ ] **Step 5:** Commit: `feat(v2): pumpkins under the PBI bar, bats, a cobweb and a spider on the Halloween theme`.

### After the tasks (controller)

1. Look at it in the real window: browser pane on the owner's 1420 vite if it is running, otherwise `tauri dev`. Check that clicks reach every ContextBar control under the pumpkins, and check reduced motion.
2. Whole-branch review.
3. Changelog entry `2.4.2` (stable), written for end users.
4. Update `docs/backlog.html`.
5. Merge, then run `scripts/release-v2.ps1 -Version 2.4.2`.

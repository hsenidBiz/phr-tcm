# Assistants Office: Three Corners Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the evenly spread desks on the shared office floor with three themed corners (Test lab, Map room, Lounge); each assistant stands in the corner matching its activity and walks across the floor when that changes.

**Architecture:** `AssistantOffice` in `src/screens/AutoRun/AssistantScene.tsx` is rewritten: it groups agents by corner, places each at a spot inside its corner, and draws the corner props. A walker's outer wrapper is positioned with `left` (with a CSS `transition` for the walk between corners); the pacing in the Map room is a `transform: translateX` keyframe on an inner element, so it never fights the transition. `AssistantsCard.tsx` is unchanged except comments if they describe the desks. Webview only.

**Tech Stack:** React 19, TS, Tailwind tokens, CSS in `src/index.css`, vitest.

**Spec:** `docs/superpowers/specs/2026-10-28-assistants-office-design.md`, section "Revision 2" (wins over everything else in it).

## Global Constraints

- Colours only from theme tokens / Tailwind token classes; corner tints and strips from `--color-success`, `--color-info`, `--color-accent` (or `color-mix` of tokens), never danger or warning. `src/ui-consistency.test.ts`, `src/a11y.test.tsx` and docs-site guards pass untouched.
- `prefers-reduced-motion: reduce`: no animation and no `left` transition (the walker jumps); poses still differ.
- The whole office stays under one `aria-hidden="true"` root; the rows, their text, `data-hue` squares and button names are unchanged.
- A `label-trim` label is never clipped (no `truncate` / overflow-hidden on it; use `overflow-x-clip text-ellipsis whitespace-nowrap`).
- No Rust, no `src/bindings.ts` edit, no new dependency, no em or en dashes in user-facing text.
- One test command at a time; heredoc commits ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; never stage `src-tauri/Cargo.toml` or `.test-cases/`.

## Review Focus

- Three replaying assistants share the Test lab: each gets its own seat and screen, no two at the same spot. Test: `spotLeft` for (lab, 0..2 of 3) gives three distinct values inside 0-33.33%.
- A Map room pacer must stay inside its corner even at the minimum card width: pacing is a fixed `translateX` of at most 2.5rem each way, and the spot is clamped to at least 2.75rem from each corner edge. Test on `spotLeft` output and the keyframe text.
- An assistant whose activity changes from discovering to replaying walks (data-moving set, then cleared after the transition), and faces left when its new spot is to its left. Test with fake timers.
- An assistant that leaves (removed from the list) and a new one that arrives do not inherit each other's movement state (key walkers by label).
- Long labels ellipsise in the name tag and do not widen the floor.

---

### Task 1: Three corners on the shared floor

**Files:**
- Modify: `src/screens/AutoRun/AssistantScene.tsx`, `src/screens/AutoRun/AssistantScene.test.tsx`, `src/index.css` (`.assistant-scene*` block and its reduced-motion list); `src/screens/AutoRun/AssistantsCard.tsx` comments only if they mention desks.

**Interfaces:**
- Keeps: `export default function AssistantOffice({ agents }: { agents: AgentView[] })`, `bubbleFor`, `deskNumber`, `poseFor`, `ScenePose`, `hueFor` (the row square uses it).
- Produces: `export type Corner = "lab" | "map" | "lounge"`; `export function cornerFor(a: AgentActivity): Corner` (replaying, working -> lab; discovering, mapping, exploring -> map; idle -> lounge); `export function spotLeft(corner: Corner, k: number, m: number): string` returning a CSS length for occupant k of m: the corner spans thirds of the floor (lab 0, map 1, lounge 2), and the spot is `clamp(<corner start> + 2.75rem, <corner start> + (k + 0.5) / m * 33.333%, <corner end> - 2.75rem)` written as a valid CSS `clamp(...)` / `calc(...)` string. `deskLeft` is removed.

- [ ] **Step 1: Tests** (rewrite the render tests; keep `bubbleFor` / `deskNumber` / `poseFor` / slot-colour tests):
  - `cornerFor` table for all seven activity kinds.
  - `spotLeft("lab",0,1)` contains `16.6` (the middle of the first third); `spotLeft("lounge",0,1)` contains `83.3`; `spotLeft("lab",k,3)` for k = 0,1,2 are three distinct strings; every result contains `2.75rem`.
  - render with agents in each corner: three `.assistant-scene__corner` elements with `data-corner` lab / map / lounge, in that order, each with its name text ("Test lab", "Map room", "Lounge"); a corner with nobody in it has `data-empty="true"`, an occupied one `data-empty="false"`.
  - each agent has one `.assistant-scene__walker` with `data-corner`, `data-pose`, `data-slot`, an inline `--spot` custom property equal to its `spotLeft` (jsdom rejects `clamp()` in an inline `left`, so the CSS rule is `left: var(--spot)`), a name tag (`.assistant-scene__tag`) with its label and class `text-ellipsis`, and the bubble only when `bubbleFor` is non-null.
  - Test lab: one `.assistant-scene__screen` per lab occupant. Lounge: a `.assistant-scene__z` per occupant. Map room: one `.assistant-scene__pin--live` exists.
  - two Map room pacers have different `animation-delay` on their `.assistant-scene__pace` elements.
  - movement (vitest fake timers): rerender an agent from `{kind:"discovering",area:"Profile"}` to a replay; its walker gets `data-moving="true"` and `data-facing="left"`; after `vi.advanceTimersByTime(2600)` `data-moving` is gone. Rerender with a different label in the same slot: no `data-moving`.
  - css: `.assistant-scene__walker` has a `transition` on `left` of 2.5s; the pace keyframes use `translateX` and `2.5rem`; the reduced-motion block sets `transition: none` on the walker and lists `.assistant-scene__pace`.
- [ ] **Step 2: Run** `npx vitest run src/screens/AutoRun/AssistantScene.test.tsx`, expect FAIL.
- [ ] **Step 3: Implement.**
  - Floor root `relative h-40 w-full overflow-hidden rounded-md border border-border bg-surface`, floor band `h-7` along the bottom. Three corner `div`s, each a third wide, holding: a tint layer (token colour at about 8 percent via `color-mix`, hidden when empty), its name (`text-[11px] font-medium uppercase tracking-wider`, token colour, muted when empty), its props (faded when empty), and a 3px strip on its floor edge (token colour, `bg-border` when empty). Two partitions at 33.333% and 66.666% (`w-[3px] bg-border-strong`, not full height).
  - Props: Test lab bench (`bottom` on the floor line) and one small screen per occupant above its seat, lit and scrolling; Map room wall map (bordered box with three pins, one `assistant-scene__pin--live` blinking) and a small plant; Lounge sofa and coffee machine. Shapes only, token colours.
  - Walker: outer `.assistant-scene__walker` absolutely positioned via inline `--spot: spotLeft(...)` and the CSS rule `left: var(--spot)`, `bottom` on the floor line, `--assistant-hue` from `hueFor(slot)`, key = label. Inside: bubble above (the current bubble classes, `max-w-[8rem]`), `.assistant-scene__pace` (Map room only, `translateX` keyframe +/-2.5rem over 6 s, inline negative delay by occupant index, svg flip on the same timing and delay), the bot svg, the name tag below (`text-[11px] text-muted max-w-[7rem] overflow-x-clip text-ellipsis whitespace-nowrap`). Lab: typing pose. Lounge: dozing pose, z beside the head.
  - Movement: a small hook tracks each label's last spot; when the spot changes, set `data-moving="true"` (legs animate, no pacing) and `data-facing` left/right from the numeric percentage part, clear it after 2500 ms; clear timers on unmount and when a label disappears. `.assistant-scene__walker { transition: left 2.5s ease-in-out }`.
  - Update both files' leading comments.
- [ ] **Step 4: Run** `npx vitest run src/screens/AutoRun src/dev src/ui-consistency.test.ts src/a11y.test.tsx docs-site`, then `npx tsc --noEmit`. Expect PASS.
- [ ] **Step 5: Commit** `feat(v2): the Assistants office has three corners, and assistants walk between them`.

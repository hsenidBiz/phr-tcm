# Assistants Office Strip (revision) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the per-assistant cubicle grid with one shared office strip across the card (one desk per assistant, walkers roam the room) above the original rows, each row gaining a colour square matching its character.

**Architecture:** `AssistantScene.tsx` keeps its file name but its default export becomes `AssistantOffice`, drawing the whole room from the agent list; the pixel character becomes an internal `Bot` component; `bubbleFor`, `deskNumber`, `poseFor` stay, `hueFor` is exported for the row square. `AssistantsCard` goes back to the row list (as at commit 83ac3112) with the office above it. Webview only.

**Tech Stack:** React 19, TS, Tailwind tokens, CSS keyframes in `src/index.css`, vitest.

**Spec:** `docs/superpowers/specs/2026-10-28-assistants-office-design.md` (read the "Revision" section first; it wins over the cubicle text).

## Global Constraints

Same as the Global Constraints of `docs/superpowers/plans/2026-10-28-assistants-office.md`: tokens only, `src/ui-consistency.test.ts` untouched, reduced motion stills all motion, office `aria-hidden`, button accessible names unchanged, no Rust / bindings / dependencies, no em or en dashes, one test command at a time, heredoc commits ending with the Co-Authored-By line, never stage `src-tauri/Cargo.toml`. Also: a `label-trim` label must not be clipped (no `truncate` / overflow-hidden on it; use `overflow-x-clip text-ellipsis whitespace-nowrap`, as the bubble does now).

## Review Focus

- Two or three walkers must not move as one figure: each walker gets a negative `animation-delay` by its index (`-(i * 2.7)s` on an 8 s walk). Test: two walkers render with different `animation-delay` values.
- A walker's bubble must not mirror when the character flips: the flip (`scaleX(-1)`) is on the svg only; the walker wrapper animates `left`. Test: the walk keyframes contain `calc(100% - 2.25rem)`, and a separate flip keyframe exists.
- One assistant: its desk is centred. Test: `deskLeft(0,1)` is `"50%"`, `deskLeft(0,2)` `"25%"`, `deskLeft(1,2)` `"75%"`.
- A long bubble ellipsises (`text-ellipsis`, `max-w-[12rem]`) and never clips descenders.
- The office is only drawn while the card is (visibility rule unchanged).

---

### Task 1: The shared office and rows

**Files:**
- Modify: `src/screens/AutoRun/AssistantScene.tsx`, `src/screens/AutoRun/AssistantScene.test.tsx`
- Modify: `src/screens/AutoRun/AssistantsCard.tsx`, `src/screens/AutoRun/AssistantsCard.test.tsx`
- Modify: `src/index.css` (the `.assistant-scene*` block and its reduced-motion list)

**Interfaces:**
- Produces: `export default function AssistantOffice({ agents }: { agents: AgentView[] })`; `export function hueFor(slot: number | null): string`; `export function deskLeft(i: number, n: number): string` returning the percentage `((i + 0.5) / n) * 100` followed by `%`. `bubbleFor`, `deskNumber`, `poseFor`, `ScenePose` unchanged.

- [ ] **Step 1: Tests.** In `AssistantScene.test.tsx` keep the `bubbleFor`, `deskNumber`, `poseFor` and slot-colour tests; add `deskLeft` cases above; rewrite the render tests against `<AssistantOffice agents={[...]} />` (build `AgentView`s the way `AssistantsCard.test.tsx` does):
  - the root has `aria-hidden="true"` and class `assistant-scene`; one `.assistant-scene__desk` per agent with `style.left` from `deskLeft`; each desk shows its number (`.assistant-scene__number`) when the label has one.
  - each agent has one `.assistant-scene__walker` with `data-pose` = `poseFor(activity)` and `data-slot`; a walking agent's desk has `assistant-scene__desk--empty`; a typing agent's desk has `.assistant-scene__screen`; a dozing agent has `.assistant-scene__z`.
  - the bubble (`.assistant-scene__bubble`) sits inside its walker, has `text-ellipsis`, and is absent when `bubbleFor` is null.
  - two walking agents get different `animation-delay` on their walkers.
  - css test: walk keyframes contain `calc(100% - 2.25rem)`; the reduced-motion block lists `.assistant-scene__walker`.
  In `AssistantsCard.test.tsx`: drop the grid / cubicle assertions; the list is a plain row list again (test: it does not have class `grid`); each row has a `[data-hue]` square whose inline background equals `hueFor(slot)`; the office renders once, above the list (exactly one `.assistant-scene`). Keep the capitalised-string tests.
- [ ] **Step 2: Run** `npx vitest run src/screens/AutoRun/AssistantScene.test.tsx src/screens/AutoRun/AssistantsCard.test.tsx`, expect FAIL.
- [ ] **Step 3: Implement.**
  - Office root: `relative h-24 w-full overflow-hidden rounded-md border border-border bg-bg`, floor `h-6` as now. Desks absolutely positioned at `deskLeft(i, n)` with `-translate-x-1/2`; the number on the desk front (`text-[11px] text-muted`, centred).
  - Walker wrapper `.assistant-scene__walker` (absolute, standing on the floor line, `--assistant-hue` set inline) holds the bubble (above the head, centred, `label-trim overflow-x-clip text-ellipsis whitespace-nowrap max-w-[12rem]` plus the bubble's current token classes) and the `Bot` svg. For type and doze it sits just left of its desk (`left: calc(<deskLeft> - 2.5rem)`); for walk the wrapper animates `left` from `0.5rem` to `calc(100% - 2.25rem)` over 8 s with an inline negative `animation-delay` by index, and the svg flips with a separate keyframe on the same timing and delay.
  - The z sits beside the dozing head. Leg / arm / eye / bob / blink animations as today.
  - `AssistantsCard`: restore the row layout from `git show 83ac3112:src/screens/AutoRun/AssistantsCard.tsx` (divide-y list, text left, buttons right), without a per-row scene, with `<AssistantOffice agents={agents} />` between the header and the list, the capitalised strings, and a square (`span`, `data-hue`, `aria-hidden`, `inline-block size-2.5 shrink-0 rounded-sm`, `style={{ background: hueFor(a.slot) }}`) before the label text. Update the leading comments in both files.
- [ ] **Step 4: Run** `npx vitest run src/screens/AutoRun src/dev src/ui-consistency.test.ts src/a11y.test.tsx docs-site`, then `npx tsc --noEmit`. Expect PASS.
- [ ] **Step 5: Commit** `feat(v2): the Assistants card shows one shared office above its rows`.

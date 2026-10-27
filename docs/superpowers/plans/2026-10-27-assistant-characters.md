# Assistant characters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each row of the Auto Run Assistants card gets a small animated pixel character whose pose shows what the assistant is doing, and the card shows only while an assistant is working.

**Architecture:** A new presentational component `AssistantScene` draws one inline-SVG character in a small scene, its pose chosen from the assistant's `AgentActivity`; CSS keyframes in `src/index.css` animate it, with a reduced-motion override. `AssistantsCard` decides visibility, puts a scene on each row, and adds a replay progress bar and a "N working" count. No Rust or bindings change.

**Tech Stack:** React 19 + TypeScript, Tailwind tokens, vitest + Testing Library (jsdom).

**Spec:** `docs/superpowers/specs/2026-10-27-assistant-characters-design.md`

## Global Constraints

- Colours only from theme tokens; `src/ui-consistency.test.ts` passes untouched. Character colours never use `danger` or `warning`.
- `prefers-reduced-motion: reduce` stops every animation (walk, bob, drift, blink); the pose still differs by activity.
- The scene and the progress bar are decorative: `aria-hidden="true"`.
- No new dependency; no Rust, bindings or `src/dev/demo.ts` change.
- No em or en dashes in added copy. Never name hidden features (Settings extras games, the held-back report module).
- Source files are CRLF: Edit/Write tools, never sed.

## Review Focus

1. An assistant list where every assistant is idle (or the list goes from working to all idle on an event): the card disappears entirely, it does not leave an empty heading.
2. A list with one working and one idle assistant: both rows show, the idle one dozing; the count says "1 working".
3. An assistant with no browser (`slot: null`) that is working: it gets a scene in the muted colour, no crash on the slot-colour lookup.
4. A replay at step 0 of 0: no progress bar (and no NaN width); at step 7 of 7, a full bar.
5. Reduced motion: the CSS has a `prefers-reduced-motion: reduce` block that turns off the scene's animations (tested as text in `src/index.css`, like any existing CSS-contract test, since jsdom does no animation).

---

### Task 1: The scene component and its animation

**Files:**
- Create: `src/screens/AutoRun/AssistantScene.tsx`
- Create: `src/screens/AutoRun/AssistantScene.test.tsx`
- Modify: `src/index.css` (beside the `ico-cog-turn` rules, ~line 901-925)

**Interfaces:**
- Produces: `export type ScenePose = "walk" | "type" | "doze";`
  `export function poseFor(a: AgentActivity): ScenePose` - discovering, mapping, exploring -> `"walk"`; replaying, working -> `"type"`; idle -> `"doze"`.
  `export default function AssistantScene(props: { activity: AgentActivity; slot: number | null }): JSX.Element` - a root element with `aria-hidden="true"`, `data-pose={pose}`, class `assistant-scene assistant-scene--{pose}`, and `data-slot` = slot or `"none"`; contains the inline SVG character (class `assistant-scene__bot`), a desk and screen (drawn for `type` only), and for `doze` a "z" element (class `assistant-scene__z`).
- Character colour: a CSS custom property set by slot on the scene, from tokens only: slot 0 -> `var(--color-accent)`, slot 1 -> `var(--color-info)`, slot 2 (and any higher) -> a third token colour that is neither danger nor warning nor the same as the first two in the default theme (builder's choice, e.g. `color-mix(in srgb, var(--color-accent) 50%, var(--color-info))`, stated in a comment), `slot: null` -> `var(--color-muted)`. The SVG fills read that property (`fill="currentColor"` with `color` set, or `fill: var(--...)`), never a hex.

- [ ] **Step 1: Write the failing tests** in `AssistantScene.test.tsx`:
  - `poseFor` maps each of the six activity kinds as above (one `test.each` with all six).
  - the rendered scene is `aria-hidden="true"` and carries `data-pose` for walk, type and doze.
  - `type` draws the desk (query by class) and `walk` / `doze` do not; `doze` has the `assistant-scene__z` element and the others do not.
  - `data-slot` is `"0"`, `"1"`, `"2"` and `"none"` for slot 0, 1, 2 and null.
  - `src/index.css` (read with `fs.readFileSync`, the way other tests read CSS in this repo; Grep for an existing one) contains keyframes used by `.assistant-scene--walk` and `.assistant-scene--type`, and a `@media (prefers-reduced-motion: reduce)` block that names `.assistant-scene` and sets `animation: none`.

- [ ] **Step 2: Run** `npx vitest run src/screens/AutoRun/AssistantScene.test.tsx` - expect FAIL (module not found).

- [ ] **Step 3: Implement** the component and the CSS. Scene about 72 x 56 px (`h-14 w-[4.5rem]`), rounded, `bg-bg` with `border-border`, a floor strip; character about 14 x 17 "pixels" drawn with `shape-rendering="crispEdges"` rects, two eyes that blink now and then. Poses: `walk` moves the character back and forth across the scene and flips it at each end; `type` sits it by the desk with a small bob; `doze` holds it still, dimmed, with the "z" drifting up and fading. The eyes' fill is a token that reads on the character (e.g. `var(--color-surface)`). Reduced-motion block turns every one of these animations off.

- [ ] **Step 4: Run** the test file again - expect PASS; then `npx vitest run src/ui-consistency.test.ts` - PASS untouched.

- [ ] **Step 5: Commit** `feat(v2): a small animated character that shows what an assistant is doing`.

### Task 2: The card shows only while an assistant works, with a scene per row

**Files:**
- Modify: `src/screens/AutoRun/AssistantsCard.tsx`
- Modify: `src/screens/AutoRun/AssistantsCard.test.tsx`
- Modify: `src/screens/AutoRun/index.test.tsx` (~1985-2029) only if it relied on an idle-only list
- Modify: `docs-site/src/content/auto-run.ts` (the Assistants card text), then `npm run docs:build`

**Interfaces:**
- Consumes: `AssistantScene`, `poseFor` from Task 1.
- Produces: `export function isWorking(a: AgentActivity): boolean` in `AssistantsCard.tsx` (true for every kind except `idle`).

- [ ] **Step 1: Write the failing tests** in `AssistantsCard.test.tsx`:
  - `"renders nothing while every assistant is idle"`: mount two idle assistants -> no heading "Assistants".
  - `"shows every assistant while one is working, the idle one dozing"`: one replaying + one idle -> both labels present; the idle row's scene has `data-pose="doze"`, the replaying row's `"type"`.
  - `"the card goes away when the last working assistant goes idle"`: fire the `autorun-agents-changed` event (the way existing tests do) moving the only working assistant to idle -> the heading disappears.
  - `"the header counts the assistants that are working"`: two working + one idle -> text `"2 working"`.
  - `"a replay shows its progress"`: step 3 of 7 -> an `aria-hidden` bar whose fill has `style.width` `"43%"` (`Math.round(3/7*100)`); step 0 of 0 -> no bar; step 7 of 7 -> `"100%"`.
  - `"a working assistant with no browser still gets a scene"`: `slot: null`, activity exploring -> scene with `data-slot="none"`.
  - Existing tests that mount only an idle assistant (the activity-text table's idle rows, the no-client/no-browser test, the button-visibility test with Assistant 3 idle) add a working companion assistant, or switch to a working activity where the assertion is not about idle; the idle-text assertions ("idle", "idle for 4 min") keep checking the idle row's text with a working assistant beside it. No assertion is dropped.

- [ ] **Step 2: Run** `npx vitest run src/screens/AutoRun/AssistantsCard.test.tsx` - expect the new tests to FAIL.

- [ ] **Step 3: Implement** in `AssistantsCard.tsx`: `if (!agents.some((a) => isWorking(a.activity))) return null;` replacing the empty-list check; each `<li>` row puts `<AssistantScene activity={a.activity} slot={a.slot} />` to the left of the text block (`flex items-center gap-3`, scene `shrink-0`); under the activity line, for `replaying` with `of > 0`, a thin bar (`h-1 w-40 rounded bg-border`, fill `bg-accent`, width `Math.round(step / of * 100)%`, `aria-hidden`); in the header beside "Assistants" a muted `text-xs` "N working". Update the file's top comment (it says the card draws nothing while no assistant is connected).

- [ ] **Step 4: Run** the card tests, `npx vitest run src/screens/AutoRun`, `npx vitest run src/ui-consistency.test.ts`, `npx tsc --noEmit` - all PASS.

- [ ] **Step 5: Guide text.** In `docs-site/src/content/auto-run.ts`, the Assistants text says the card appears while an assistant is working (discovering, mapping menus, exploring or replaying) and that each row's small figure walks while it explores, types while it replays, and dozes when idle. Then `npm run docs:build` and `npx vitest run docs-site` - PASS (no screenshot change in this task).

- [ ] **Step 6: Commit** `feat(v2): the Assistants card shows only while an assistant works, with a figure on each row`.

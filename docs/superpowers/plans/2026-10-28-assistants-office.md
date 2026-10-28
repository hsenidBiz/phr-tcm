# Assistants Office Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the Assistants card's rows into side-by-side cubicles, each with a bigger animated scene (desk number, speech bubble) above the row's full text and buttons, and capitalize the browser and activity lines.

**Architecture:** `AssistantScene` grows from the 72 x 56 px row figure into a full-width cubicle scene, keeping its inline SVG character and CSS keyframes. `AssistantsCard` lays the assistants out in a responsive grid, each cell the scene with today's text, badges, bar, buttons and status lines below. Webview only.

**Tech Stack:** React 19, TypeScript, Tailwind tokens, CSS keyframes in `src/index.css`, vitest + Testing Library.

**Spec:** `docs/superpowers/specs/2026-10-28-assistants-office-design.md`

## Global Constraints

- Colours only from theme tokens; `src/ui-consistency.test.ts` passes untouched (never weaken it).
- `prefers-reduced-motion: reduce` stills every animation (extend the existing `@media` block in `src/index.css`); poses still look different.
- Scene, bubble and desk number are decorative: everything inside the scene stays under its `aria-hidden="true"` root.
- Button accessible names unchanged: `Stop <label>`, `End discovery for <label>`, `Release <label>'s browser`.
- No Rust, no `src/bindings.ts` edit, no new dependency. No em or en dashes in user-facing text.
- Card visibility rule unchanged (renders nothing while no assistant is working).
- Run one test command at a time; never set `CARGO_TARGET_DIR`. Commit with a bash heredoc (`git commit -F - <<'EOF'`), ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

- A long area name ("Leave Encashment Approvals") in the bubble must not overflow the cubicle: the bubble truncates with an ellipsis (`max-width` + `truncate`). Test: bubble has the `truncate` class.
- A label with no number ("Assistant" or anything not ending in digits) shows no desk number rather than "NaN" or "undefined". Test in Task 1.
- One assistant alone must not stretch into a full-card-width cubicle that looks empty: the grid uses `repeat(auto-fill, minmax(12rem, 1fr))` (auto-fill, not auto-fit), so a lone cubicle keeps its column width. Test in Task 2 (class assertion).
- The walk must stay inside the cubicle at any width: animate `left` between `0.5rem` and `calc(100% - 2.25rem)` (percent of the scene), not a fixed px translate. Test: css text contains `calc(100% - 2.25rem)` in the walk keyframes.
- Replay at step 0 of 0 says "Starting" in the bubble, never "Step 0 of 0". Test in Task 1.

---

### Task 1: The cubicle scene

**Files:**
- Modify: `src/screens/AutoRun/AssistantScene.tsx`
- Modify: `src/index.css` (the `.assistant-scene*` block, ~lines 920-995)
- Test: `src/screens/AutoRun/AssistantScene.test.tsx`

**Interfaces:**
- Produces: `export default function AssistantScene({ activity, slot, label }: { activity: AgentActivity; slot: number | null; label: string })`; `export function bubbleFor(a: AgentActivity): string | null`; `export function deskNumber(label: string): string | null`. `poseFor` and `ScenePose` unchanged.

- [ ] **Step 1: Write the failing tests** in `AssistantScene.test.tsx` (keep every existing test; update any that pass no `label` to pass `label="Assistant 1"`, and replace the old 72 x 56 size expectations if any):

```ts
test.each<[AgentActivity, string | null]>([
  [{ kind: "replaying", case_id: 4521, step: 3, of: 7 }, "Step 3 of 7"],
  [{ kind: "replaying", case_id: 4521, step: 0, of: 0 }, "Starting"],
  [{ kind: "discovering", area: "Profile" }, "Profile"],
  [{ kind: "discovering", area: null }, null],
  [{ kind: "mapping", modules: ["Leave"] }, "Menus"],
  [{ kind: "exploring" }, null],
  [{ kind: "working" }, null],
  [{ kind: "idle" }, null],
])("bubbleFor %o is %s", (a, text) => expect(bubbleFor(a)).toBe(text));

test.each([["Assistant 2", "2"], ["Assistant 12", "12"], ["Assistant", null], ["", null]])(
  "deskNumber(%s) is %s", (l, n) => expect(deskNumber(l)).toBe(n));

test("the scene shows the bubble and desk number, truncating a long bubble", () => {
  const { container } = render(<AssistantScene activity={{ kind: "discovering", area: "Leave Encashment Approvals" }} slot={1} label="Assistant 2" />);
  const bubble = container.querySelector(".assistant-scene__bubble")!;
  expect(bubble).toHaveTextContent("Leave Encashment Approvals");
  expect(bubble).toHaveClass("truncate");
  expect(container.querySelector(".assistant-scene__number")).toHaveTextContent("2");
});

test("no bubble or number when there is nothing to say", () => {
  const { container } = render(<AssistantScene activity={{ kind: "idle" }} slot={0} label="Assistant" />);
  expect(container.querySelector(".assistant-scene__bubble")).toBeNull();
  expect(container.querySelector(".assistant-scene__number")).toBeNull();
});

test("walking leaves the desk in the cubicle, faded", () => {
  const { container } = render(<AssistantScene activity={{ kind: "exploring" }} slot={0} label="Assistant 1" />);
  expect(container.querySelector(".assistant-scene__desk")).toHaveClass("assistant-scene__desk--empty");
});
```

Extend the existing css test ("the css animates the walk...") to also expect the walk keyframes to contain `calc(100% - 2.25rem)` and the reduced-motion block to list `.assistant-scene__bubble` if the bubble animates.

- [ ] **Step 2: Run** `npx vitest run src/screens/AutoRun/AssistantScene.test.tsx` — expect the new tests to FAIL (missing exports / elements).

- [ ] **Step 3: Implement.**
  - `bubbleFor` returns exactly the strings in the test. `deskNumber` returns the trailing digits of the label (`/(\d+)$/`) or null.
  - Root: `w-full h-24` (96 px), keeps `aria-hidden`, `data-pose`, `data-slot`, the classes `assistant-scene assistant-scene--${pose}`, border/bg tokens. Floor ~1.5rem tall (`bg-surface-2`, `border-t border-border`).
  - Desk always drawn, centred (`left-1/2 -translate-x-1/2`), with its screen; the screen shows when the pose is `type`; while walking add `assistant-scene__desk--empty` (CSS: `opacity: 0.5`). The character sits at the desk for `type` and `doze` (left of centre), and walks for `walk`.
  - Bubble: top-left, `assistant-scene__bubble truncate max-w-[70%] rounded border border-border bg-surface px-1.5 text-[11px] text-text`. Number: top-right, `assistant-scene__number text-[11px] text-muted`.
  - CSS: the walk animates `left` from `0.5rem` to `calc(100% - 2.25rem)` with the flip at each end (`scaleX(-1)` via transform), about 8 s; typing bob and doze "z" as today, repositioned beside the desk. Update the block's leading comment to the new sizes.

- [ ] **Step 4: Run** `npx vitest run src/screens/AutoRun/AssistantScene.test.tsx src/ui-consistency.test.ts` — expect PASS.

- [ ] **Step 5: Commit** `feat(v2): the assistant scene becomes a cubicle with a desk number and a speech bubble`.

### Task 2: The card as a grid of cubicles, capitalized lines

**Files:**
- Modify: `src/screens/AutoRun/AssistantsCard.tsx`
- Test: `src/screens/AutoRun/AssistantsCard.test.tsx`; any capitalised-text expectation in `src/screens/AutoRun/index.test.tsx`, `src/screens/AutoRun.test.tsx`, `src/dev/demoAssistants.test.ts`
- Modify: `docs-site/src/content/auto-run.ts` (the `ASSISTANTS` shot `alt` and the `assistants` group summary, only if they describe rows)

**Interfaces:**
- Consumes: `AssistantScene({ activity, slot, label })` from Task 1.

- [ ] **Step 1: Update the tests** in `AssistantsCard.test.tsx`: every expected browser/activity string starts with a capital, e.g. `"Main Auto Run browser, Edge, signed in as hr_admin"`, `"Its own browser"`, `"No Auto Run browser"`, `"Replaying case 4521 (step 3 of 7)"`, `"Replaying case 4521 (starting)"`, `"Discovering Profile"`, `"Discovering"`, `"Mapping menus (Leave, Payroll)"`, `"Exploring"`, `"Working"`, `"Idle"`, `"Idle for 4 min"`. Add:

```ts
test("each assistant is a cubicle in a grid that keeps its width when alone", async () => {
  // render with one working assistant, as the existing tests do
  const list = await screen.findByRole("list");
  expect(list).toHaveClass("grid");
  expect(list.className).toContain("auto-fill");
  expect(list.querySelectorAll(":scope > li")).toHaveLength(1);
  expect(list.querySelector("li .assistant-scene")).not.toBeNull();
});
```

- [ ] **Step 2: Run** `npx vitest run src/screens/AutoRun/AssistantsCard.test.tsx` — expect FAIL.

- [ ] **Step 3: Implement.** Capitalize the strings in `activityText` and `browserText` (only the first letter of each; the rest of each sentence unchanged). The `<ul>` becomes `grid gap-4 [grid-template-columns:repeat(auto-fill,minmax(12rem,1fr))]` with no dividers; each `<li>` is a column: `<AssistantScene activity slot label={a.label} />`, then the existing text block (label line, browser line, activity line + badges, progress bar now `w-full`), then the buttons row (`flex flex-wrap gap-2`), then the existing status / alert paragraphs unchanged. Update the file's leading comment (cubicles, not rows).

- [ ] **Step 4: Run** `npx vitest run src/screens/AutoRun src/dev src/ui-consistency.test.ts src/a11y.test.tsx docs-site` then `npx tsc --noEmit` — expect PASS. Fix any other test that expected the lower-case strings.

- [ ] **Step 5: Commit** `feat(v2): the Assistants card lays assistants out as cubicles; status lines start with a capital`.

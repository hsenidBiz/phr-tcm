# Assistants card: an office of cubicles

Owner request, 2026-10-28: make the Assistants card on Auto Run more fun, as a shared office with desks; and capitalize the status lines that start lower case.

## Intent

The card stays the at-a-glance view of what each assistant is doing, but reads as a little office: each assistant has its own cubicle and you watch it work. Nothing it tells a person today is lost.

## Decided with the owner

- **Layout: the hybrid of mockups A and B.** The card's rows become cubicles sitting side by side (a responsive grid, about 190 px minimum per cubicle, wrapping on a narrow window). Each cubicle is a scene (about 96 px tall, full cubicle width) with the assistant's numbered desk and its character. Under the scene, everything a row shows today: the label line, the full browser line, the full activity line with its badges, the replay progress bar, and the Stop / End discovery / Release buttons, plus the row's status / alert lines.
- **Poses**, as today but in the bigger scene: replaying or working sits at its desk typing; discovering, mapping or exploring walks back and forth across the cubicle, turning at each end, with its desk empty; idle dozes at its desk, dimmed, with a drifting "z".
- **A speech bubble** in the scene's top-left says the short version of the activity: "Step 3 of 7" (or "Starting") while replaying, the area name while discovering an area, "Menus" while mapping. None for idle, working, exploring, or discovering with no area (my call: the bubble only appears when it adds something short).
- **The desk number** (the assistant's own number from its label, "Assistant 2" -> 2) sits in the scene's top-right.
- **Capitals:** the browser line and activity line start with a capital: "Main Auto Run browser", "Its own browser", "No Auto Run browser", "Idle", "Idle for 4 min", "Discovering Profile", "Discovering", "Mapping menus (...)", "Exploring", "Replaying case 4521 (step 3 of 7)", "Working". The client name ("claude-code") is what the assistant reports and is shown as is.

## Approaches

1. **Keep the inline SVG character and CSS keyframes, rebuild the scene as a cubicle** (chosen). Theme tokens, reduced motion and no dependency all carry over.
2. Sprite images: colours baked in, breaks theming. Rejected earlier, still rejected.
3. Canvas or an animation library: a redraw loop and a dependency for a few figures. Rejected.

## Constraints

- Colours only from theme tokens; `src/ui-consistency.test.ts` passes untouched. Character colour per browser slot stays as today (`hueFor`).
- `prefers-reduced-motion: reduce` stills every animation; poses still differ.
- The scene, bubble and number are decorative (`aria-hidden`); the text lines below already say it all. Button accessible names are unchanged.
- No Rust or bindings change. No new dependency. No em or en dashes in user-facing text.
- The card's visibility rule (shown only while an assistant is working) is unchanged.

## Left to the builder

- Exact sizes, timings, and how the walk spans the cubicle width (percent-based keyframes).
- Whether the cubicle is a new component or `AssistantScene` grows into it.

## Touches

- `src/screens/AutoRun/AssistantScene.tsx` (+ test), `AssistantsCard.tsx` (+ test), `src/index.css` (`.assistant-scene*`).
- `src/dev/demo.ts` sample text if it hard-codes lower-case lines.
- `src/lib/changelog.ts`: one end-user line in the next beta.
- How To Use: the Assistants screenshot (`auto-run-assistants`) is retaken; the tip text in `docs-site/src/content/auto-run.ts` mentions the office if it describes the rows.

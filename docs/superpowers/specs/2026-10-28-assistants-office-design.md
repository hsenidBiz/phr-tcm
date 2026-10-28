# Assistants card: an office of cubicles

Owner request, 2026-10-28: make the Assistants card on Auto Run more fun, as a shared office with desks; and capitalize the status lines that start lower case.

## Intent

The card stays the at-a-glance view of what each assistant is doing, but reads as a little office: each assistant has its own cubicle and you watch it work. Nothing it tells a person today is lost.

## Revision 2, 2026-10-28 (owner, from a reference of themed rooms): three corners on one floor

Wins over everything below. The office stays one shared floor across the full card width (about 160 px tall, `aria-hidden`), with the rows below it exactly as in Revision 1 (colour square, capitalised lines, buttons). The floor is split by two low partitions into three equal corners, each named in small caps at its top-left:

| Corner | Who is there | Props |
|---|---|---|
| **Test lab** (left) | replaying, working | a bench with one screen per occupant, lit and scrolling |
| **Map room** (middle) | discovering, mapping, exploring | a wall map with pins, one pin blinking; a plant |
| **Lounge** (right) | idle | a sofa, a coffee machine |

- Occupants of a corner are spread evenly across it, in the card's order. In the Test lab they sit at the bench typing; in the Map room each paces a short stretch (about 2.5rem each way) around its own spot, never leaving the corner, with several pacers out of step; in the Lounge they doze on the sofa, dimmed, with a drifting "z".
- A corner with anyone in it lights up (a light tint and a coloured strip along its floor edge, from theme tokens, never danger or warning); an empty corner is dimmed with its props faded.
- Each character has a name tag under it (its label, ellipsised if long) and the speech bubble above it (strings as below; "Working" has no bubble).
- When an assistant's corner changes, it walks across the floor to its new spot (about 2.5 s, legs moving, facing the way it goes) rather than jumping. Under reduced motion it jumps and nothing moves.

Revision 1 and the cubicle text are kept for the record.

## Revision, 2026-10-28 (owner, after seeing the cubicles on a wide screen)

The cubicle grid left most of a wide card empty. **Switched to mockup A:** one shared office strip across the full width of the card, with the rows below it as they were before this change (label, browser line, activity line with badges, replay bar, buttons on the right, status / alert lines), plus a small square before each row's label in that assistant's character colour, so a row and its character are matched at a glance. Capitals stay as below.

The office (about 96 px tall, full card width, `aria-hidden`):
- One desk per assistant in the card's order, spread evenly: desk *i* of *n* centred at `(i + 0.5) / n` of the width. The desk number (from the label) is on the desk's front.
- Replaying or working: the assistant sits at its desk, typing, its screen lit. Idle: dozes at its desk, dimmed, "z" drifting. Discovering, mapping, exploring: it paces a short stretch around its own desk (about 5rem to its left to 3rem to its right, kept inside the room, about 6 s per round trip), its desk faded; several walkers start at different points of the walk so they do not move as one.
- The speech bubble (same strings as below) floats above the character's head and moves with it; the character flips at each end of the walk, the bubble does not.

The cubicle-grid text below is kept for the record; where it conflicts, this revision wins.

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

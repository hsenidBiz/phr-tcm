# Assistants card: a small animated character per assistant

Owner request, 2026-10-27 (with a reference picture of a pixel-art "agent office"): show a 2D character working while an assistant runs an Auto Run task, with several assistants at once, and do not show the Assistants area until an assistant is working.

## Intent

At a glance, a person watching the Auto Run screen sees which assistants are busy and what kind of work each is doing, without reading every line. The card stays out of the way when there is nothing to watch.

## Decided with the owner

- **Layout: Option B.** Today's Assistants card and rows stay as they are (label, browser line, activity line, badges, Stop / End discovery / Release on the right). Each row gains a small scene on its left (about 72 x 56 px) with a pixel character in it. Option A (one room per browser) was shown and not chosen.
- **When the card shows:** only while at least one connected assistant is doing something: discovering, mapping menus, exploring, replaying a case, or working. When every connected assistant is idle (or none is connected) the card draws nothing. While the card shows, an idle assistant keeps its row (dozing), so its browser can still be released.
- **How it is drawn: inline SVG pixel character, animated with CSS** (approach 1 of 3). Rejected: sprite-sheet images (colours baked in, breaks theming and the no-hardcoded-colour rule, frames to draw) and a canvas loop or animation library (redraws many times a second, a new dependency, heavy for three figures).

## Poses (from `AgentActivity.kind`)

| Activity | Pose |
|---|---|
| discovering, mapping, exploring | walks back and forth across its scene, turning at each end |
| replaying, working | sits at a small desk with a screen and types (a small bob) |
| idle | stands still, dimmed, with a "z" drifting up |

Replaying also shows a thin progress bar under the activity line, at step / of. No bar before the first step (0 of 0), matching the existing "(starting)" text.

The card header gains a muted count, "N working", counting assistants whose activity is not idle.

## Constraints

- **Colours only from theme tokens** (`src/ui-consistency.test.ts` must pass untouched). One character colour per browser slot so two assistants are told apart: slot 0 (main browser) and the two extra slots each take a distinct token colour. Never `danger` or `warning`, which mean a problem. An assistant with no browser uses the muted text colour. The scene's floor, desk and screen use surface / border tokens.
- **Reduced motion:** with `prefers-reduced-motion: reduce` every pose is still (no walk, bob, drift or blink); the pose itself still differs by activity. Follow the pattern of the existing `ico-cog-turn` rules in `src/index.css`.
- **Accessibility:** the scene is decorative (`aria-hidden`); the activity line already says what the assistant is doing. The progress bar is decorative too (the step text is already there).
- No new dependency. No change to Rust or the bindings: everything comes from the existing `AgentView` list.
- No em or en dashes in user-facing text; the changelog line is end-user-facing.

## Left to the builder

- The exact pixel drawing (one small character, roughly 14 x 17 "pixels", with eyes that blink now and then) and animation timings, within the poses above.
- Which three token colours the slots take, within the constraint above.
- Whether the scene is its own small component file beside `AssistantsCard.tsx`.

## Touches

- `src/screens/AutoRun/AssistantsCard.tsx`: the visibility rule, the scene per row, the progress bar, the header count. Its tests in `AssistantsCard.test.tsx` (hidden when all idle or none; shown when any is working; each activity maps to its pose class; bar width and the no-bar case; scene is `aria-hidden`), and any test elsewhere (`AutoRun/index.test.tsx`, `AutoRun.test.tsx`) that expected the card with only idle assistants.
- `src/index.css`: the keyframes and reduced-motion rules.
- `src/lib/changelog.ts`: one end-user line in the next beta.
- How To Use (`docs-site/src/content/auto-run.ts`): the Assistants tip says the card appears while an assistant is working and shows each one's character. The pending Assistants screenshot (the final screenshot run, `docs/final-shots`) is taken after this ships, so it shows the characters.

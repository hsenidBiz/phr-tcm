# Halloween theme - design (short)

**Intent.** A fun seasonal look for Halloween 2026. The update switches
everyone onto it for a while, and they can always pick something else in
Settings.

**Owner's decisions:**
- The palette is orange and black. The accent changes to orange too,
  because otherwise the default violet accent covers the theme's own
  colour.
- Pumpkins hang from just under the PBI bar (`ContextBar`), with bats too.
  It should be fun.
- Everyone who updates is switched onto it, and Settings can override it.
- People keep it for a week, then they go back automatically.
- It ships as a stable release, so every install gets it.

**Approach.** There is really only one sensible way. The app already has a
theme system (`src/lib/theme.ts` plus `data-theme` blocks in
`src/index.css`), so Halloween becomes a 7th entry in `THEMES`, with
`dark: true` and every colour token defined. The decorations are a small
SVG/CSS component that shows only while `data-theme="halloween"` is set.
Building the decorations as a separate overlay that ignores the theme
would keep them on screen after someone switches theme, so that option was
dropped.

**How the switch works.** These are my calls; please check them.
- On first launch after the update, a one-time flag in localStorage
  remembers the person's current theme choice and accent. It then sets
  Halloween with the accent "default", so the theme's orange shows. The
  switch only happens if that launch is before 2026-11-01. Anyone updating
  later is never switched.
- "A week" means 7 days from the moment that machine was switched. After
  that, and only if the person is still on Halloween and hasn't touched
  the theme or accent since, the stored theme and accent come back.
- Once someone picks a theme or accent themselves, the timer is cancelled
  and their choice stands. That includes re-picking Halloween.
- Halloween stays in the Settings theme list after the week, so anyone
  who wants it can keep it.

**What it looks like.**
- **Colours.** Near-black backgrounds and surfaces with a faint purple
  tint, pumpkin orange `#f97316` as the accent, warm off-white text. The
  status colours (success, danger, warning) stay readable. The a11y and
  contrast gates must still pass.
- **Pumpkins.** 3 to 5 jack-o'-lanterns hang on strings of different
  lengths from the bottom edge of the PBI bar. They sway gently and their
  faces glow. They take no clicks (`pointer-events: none`), sit in the
  bar's empty right-hand space, and never cover its controls.
- **Bats.** A few small bats flap across the top of the window now and
  then.
- **Corner extras.** A cobweb in one corner, and a spider that drops on
  its thread now and again.
- **Reduced motion.** When reduced motion is on, every animation stops
  and the decorations stay still.
- **Settings.** A Halloween card in the theme picker, with orange and
  black swatches.
- **Changelog.** A stable entry, written for end users: Halloween theme,
  on for a week, change it any time in Settings.

**Non-goals.** No sound, no change to the guide's screenshots, nothing in
the Auto Run browser pages.

**Anti-goals.** Decorations must never block clicks or hide content (the
jsdom tests cannot catch overlay hit-testing, so this needs a manual check
in the real window). People must never be forced back onto the theme after
they've left it.

**Left to the builder.** The exact SVG art, the timings, and the token
values, within the contrast gates.

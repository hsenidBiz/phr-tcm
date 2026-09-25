// The keyboard focus ring. Most controls never style their own, so the
// browser's default showed - a dark ring that faded in from white, because
// Tailwind's `transition-colors` also animates outline-color and the
// unfocused outline colour was the (light) text colour. One rule in the
// lowest layer makes every ring the theme's accent, and gives every element
// that same outline colour before it is focused, so there is nothing to
// fade from. Being the lowest layer, any component's own focus style
// (Astryx's, XiodUI's, a `focus-visible:` utility) still wins for the
// properties it sets.
//
// Astryx's reset sits in the same layer and drops the ring on touch-only
// devices and on a <dialog>. A later rule of equal weight would undo both,
// so these tests also pin that the app's ring leaves those two alone.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";

const css = readFileSync(resolve(__dirname, "index.css"), "utf8");

/** The `{ ... }` block that starts at the first `opener` in `source`. */
function block(source: string, opener: string): string {
  const at = source.indexOf(opener);
  expect(at, `found ${opener} ... { ... }`).toBeGreaterThan(-1);
  let depth = 0;
  for (let i = source.indexOf("{", at); i < source.length; i++) {
    if (source[i] === "{") depth++;
    if (source[i] === "}" && --depth === 0) return source.slice(at, i + 1);
  }
  return "";
}

const resetLayer = () => block(css, "@layer reset {");

// Astryx's own rule is `@media (hover: none) and (pointer: coarse)`; this
// is its exact reverse.
const NOT_TOUCH_ONLY = "@media not all and (hover: none) and (pointer: coarse) {";

test("the focus ring is the theme accent, set in the lowest layer", () => {
  const reset = resetLayer();
  expect(reset).toMatch(/:where\(:focus-visible\)\s*\{[^}]*outline:\s*2px solid var\(--color-accent\)/);
});

test("every element already carries the accent outline colour, so focusing it has no colour to fade from", () => {
  expect(resetLayer()).toMatch(/:where\(\*\)\s*\{[^}]*outline-color:\s*var\(--color-accent\)/);
});

test("the ring is only drawn off touch-only devices, where Astryx's reset suppresses it", () => {
  const reset = resetLayer();
  const media = block(reset, NOT_TOUCH_ONLY);
  expect(media).toMatch(/:where\(:focus-visible\)\s*\{[^}]*outline:\s*2px solid/);
  // Nowhere else in the layer: a copy outside the media block would draw
  // the ring on a touch-only device again.
  const outside = reset.replace(media, "");
  expect(outside).not.toMatch(/outline:\s*2px solid/);
  expect(outside).not.toMatch(/outline-offset/);
});

test("a dialog keeps Astryx's no-outline, even when it has focus itself", () => {
  const reset = resetLayer();
  expect(reset).toMatch(/:where\(dialog:focus-visible\)\s*\{[^}]*outline:\s*none/);
  // After the ring, so it wins at equal weight.
  expect(reset.indexOf(":where(dialog:focus-visible)")).toBeGreaterThan(reset.indexOf(NOT_TOUCH_ONLY));
});

test("the ring's gap skips a control that styles its own outline, so its ring is not pushed out", () => {
  const media = block(resetLayer(), NOT_TOUCH_ONLY);
  expect(media).toMatch(/:where\(:focus-visible:not\(\[class\*="outline"\]\)\)\s*\{[^}]*outline-offset:\s*2px/);
  // The ring rule itself sets no offset for every element.
  expect(media).not.toMatch(/:where\(:focus-visible\)\s*\{[^}]*outline-offset/);
});

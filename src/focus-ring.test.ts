// The keyboard focus ring. Most controls never style their own, so the
// browser's default showed - a dark ring that faded in from white, because
// Tailwind's `transition-colors` also animates outline-color and the
// unfocused outline colour was the (light) text colour. One rule in the
// lowest layer makes every ring the theme's accent, and gives every element
// that same outline colour before it is focused, so there is nothing to
// fade from. Being the lowest layer, any component's own focus style
// (Astryx's, XiodUI's, a `focus-visible:` utility) still wins.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";

const css = readFileSync(resolve(__dirname, "index.css"), "utf8");

function resetLayer(): string {
  const at = css.indexOf("@layer reset {");
  expect(at, "index.css has an @layer reset { ... } block").toBeGreaterThan(-1);
  let depth = 0;
  for (let i = css.indexOf("{", at); i < css.length; i++) {
    if (css[i] === "{") depth++;
    if (css[i] === "}" && --depth === 0) return css.slice(at, i + 1);
  }
  return "";
}

test("the focus ring is the theme accent, set in the lowest layer", () => {
  const reset = resetLayer();
  expect(reset).toMatch(/:where\(:focus-visible\)\s*\{[^}]*outline:\s*2px solid var\(--color-accent\)/);
});

test("every element already carries the accent outline colour, so focusing it has no colour to fade from", () => {
  expect(resetLayer()).toMatch(/:where\(\*\)\s*\{[^}]*outline-color:\s*var\(--color-accent\)/);
});

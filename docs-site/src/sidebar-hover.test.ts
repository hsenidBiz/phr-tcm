// A screen's subsections open in the sidebar on hover and on keyboard focus,
// not only while the screen is the one being read, so a reader can jump
// straight into one. jsdom has no :hover, so this pins the rules and the
// per-list link count the open height is sized from.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";
import { renderSidebar } from "./render/sidebar";
import { screens, recipes, intro } from "./content";

const css = readFileSync(resolve(__dirname, "styles.css"), "utf8");

test("hover and focus open a screen's subsections, as reading it does", () => {
  const open = css.match(/li\[data-open\] > \.nav-sub,\s*li:hover > \.nav-sub,\s*li:focus-within > \.nav-sub\s*\{([^}]*)\}/);
  expect(open, "the open rule covers data-open, hover and focus-within").not.toBeNull();
  expect(open![1]).toMatch(/visibility:\s*visible/);
  // Collapsed lists are hidden, so their links are out of the tab order.
  expect(css).toMatch(/\.nav-sub \{[^}]*visibility:\s*hidden/);
});

test("each subsection list carries its link count", () => {
  const nav = renderSidebar({ screens, recipes, intro, available: [] } as never);
  const lists = [...nav.querySelectorAll<HTMLUListElement>(".nav-sub")];
  expect(lists.length).toBeGreaterThan(0);
  for (const ul of lists) {
    expect(ul.getAttribute("style")).toBe(`--n: ${ul.querySelectorAll("a").length}`);
  }
});

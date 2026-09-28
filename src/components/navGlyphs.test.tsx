// The sidebar's own glyphs: every row draws one, every part that
// moves is named for the stylesheet, and none of that motion exists for
// anyone who asked the OS for less.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { render } from "@testing-library/react";
import { expect, test } from "vitest";
import { CASE_ITEMS, WORK_ITEMS } from "./Sidebar";

const css = readFileSync(resolve(__dirname, "../index.css"), "utf8");

test("every sidebar row, test-case and Work Manager, draws its own glyph", () => {
  // Update Test Cases' glyph is the update arrow; every other row's is named for its id.
  const kind = (id: string) => (id === "edit" ? "update" : id);
  for (const { id, icon: Icon } of [...CASE_ITEMS, ...WORK_ITEMS]) {
    const { container, unmount } = render(<Icon size={16} className="nav-ico" />);
    const svg = container.querySelector("svg")!;
    expect(svg.classList.contains(`ng-${kind(id)}`), id).toBe(true);
    expect(svg.getAttribute("aria-hidden"), id).toBe("true");
    unmount();
  }
});

test("clip ids are unique per glyph, so two copies never share a clip", () => {
  const View = CASE_ITEMS.find((i) => i.id === "view")!.icon;
  const { container } = render(
    <>
      <View />
      <View />
    </>,
  );
  const ids = [...container.querySelectorAll("clipPath")].map((c) => c.id);
  expect(ids).toHaveLength(2);
  expect(new Set(ids).size).toBe(2);
  for (const id of ids) expect(id).toMatch(/^[a-zA-Z0-9-]+$/);
});

test("glyph motion only runs when the OS has not asked for less", () => {
  const start = css.indexOf("@media (prefers-reduced-motion: no-preference)");
  expect(start).toBeGreaterThan(-1);
  // Every rule that starts an ng- animation sits inside that block.
  const before = css.slice(0, start);
  expect(before).not.toMatch(/\.ng-[a-z-]+[^{]*\{[^}]*animation:/);
  let depth = 0;
  let end = css.indexOf("{", start);
  for (let i = end; i < css.length; i++) {
    if (css[i] === "{") depth++;
    else if (css[i] === "}" && --depth === 0) {
      end = i;
      break;
    }
  }
  expect(css.slice(end)).not.toMatch(/\.ng-[a-z-]+[^{]*\{[^}]*animation:/);
});

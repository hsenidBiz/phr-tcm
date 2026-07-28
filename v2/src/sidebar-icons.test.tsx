/**
 * The sidebar's two-tone icons are painted by matching lucide's own path
 * data in CSS - `.nav-ico-create path[d="M14 19h6"]`. That is deliberate:
 * an nth-child rule would silently paint the WRONG stroke after an icon is
 * redrawn, whereas a `d` selector simply stops matching and the glyph
 * falls back to one colour.
 *
 * "Simply stops matching" is still a silent failure though, and it had
 * already happened: a lucide bump redrew the new-work-item glyph and the
 * plus quietly lost its green. Nothing failed, nothing was logged, the
 * icon just went monochrome.
 *
 * So this test renders each icon the sidebar actually uses and checks
 * every selector still hits something. It reads the pairing out of
 * Sidebar.tsx rather than restating it, so swapping an icon there is
 * enough to re-point the check.
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { renderToStaticMarkup } from "react-dom/server";
import { createElement } from "react";
import * as Lucide from "lucide-react";
import { expect, test } from "vitest";

const SRC = dirname(fileURLToPath(import.meta.url));
const css = readFileSync(join(SRC, "index.css"), "utf-8");
const sidebar = readFileSync(join(SRC, "components/Sidebar.tsx"), "utf-8");

/** nav id -> the lucide component the sidebar renders for it. */
function iconsByNavId(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of sidebar.matchAll(
    /icon:\s*(\w+),\s*tone:\s*"nav-ico nav-ico-(\w+)"/g,
  )) {
    out[m[2]] = m[1];
  }
  return out;
}

/** nav id -> every path `d` the stylesheet targets for it. */
function selectorsByNavId(): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  for (const m of css.matchAll(/\.nav-ico-(\w+)\s+path\[d="([^"]+)"\]/g)) {
    (out[m[1] ??= m[1]] ||= []).push(m[2]);
  }
  return out;
}

/** `.nav-ico-on` is the selected-row STATE, not a nav item - it flattens
 * every glyph to the accent so the highlight reads as one block. It has no
 * icon of its own to check against. */
const STATE_CLASSES = new Set(["on"]);

const icons = iconsByNavId();
const selectors = selectorsByNavId();

test("every sidebar item's icon is resolvable", () => {
  expect(Object.keys(icons).length).toBeGreaterThanOrEqual(10);
  for (const [id, name] of Object.entries(icons)) {
    expect(Lucide, `${id} uses an icon lucide does not export: ${name}`).toHaveProperty(name);
  }
});

test("every two-tone selector still matches a path in its icon", () => {
  // The rules exist to colour something; a stylesheet that targets nothing
  // is the bug this test was written for.
  expect(Object.keys(selectors).length).toBeGreaterThan(0);

  const stale: string[] = [];
  for (const [id, paths] of Object.entries(selectors)) {
    if (STATE_CLASSES.has(id)) continue;
    const name = icons[id];
    expect(name, `index.css paints .nav-ico-${id}, which no sidebar item uses`).toBeTruthy();
    const svg = renderToStaticMarkup(
      createElement(Lucide[name as keyof typeof Lucide] as never),
    );
    for (const d of paths) {
      if (!svg.includes(`d="${d}"`)) stale.push(`.nav-ico-${id} path[d="${d}"] (${name})`);
    }
  }
  expect(stale, "these selectors no longer match - the icon was redrawn").toEqual([]);
});

/** The parts that are coloured by element type rather than by path data.
 * Cheaper to keep honest, but they still have to hit something. */
test("the element-level selectors match too", () => {
  const byElement = [...css.matchAll(/\.nav-ico-(\w+)\s+(circle|rect)\s*\{/g)];
  expect(byElement.length).toBeGreaterThan(0);
  for (const m of byElement) {
    const [, id, element] = m;
    if (STATE_CLASSES.has(id)) continue;
    const name = icons[id];
    expect(name, `index.css paints .nav-ico-${id}, which no sidebar item uses`).toBeTruthy();
    const svg = renderToStaticMarkup(
      createElement(Lucide[name as keyof typeof Lucide] as never),
    );
    expect(svg, `.nav-ico-${id} ${element} matches nothing in ${name}`).toContain(`<${element}`);
  }
});

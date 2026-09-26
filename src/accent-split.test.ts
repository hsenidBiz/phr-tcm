// The accent does two jobs: it FILLS (buttons, selected borders, switches,
// focus rings, the brand flask) and it colours WORDS. In every theme and
// accent those are the same colour, except dark violet: the icon's vivid
// violet carries white button text but is too dark to read as text on the
// dark surfaces, and the lighter shade that reads well looked washed out
// as a fill. So dark violet sets its fill and its text shade apart, and
// `text-accent` reads the text shade (index.css, @theme).
//
// These tests pin the values and re-derive the contrast from the CSS, so a
// later tweak to a surface or to the violet cannot quietly drop below AA.

import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const SRC = dirname(fileURLToPath(import.meta.url));
const css = readFileSync(resolve(SRC, "index.css"), "utf8");
const bridge = readFileSync(resolve(SRC, "xiod-theme.css"), "utf8");

/** The declarations of the first rule whose selector is exactly `selector`. */
function rule(source: string, selector: string): Record<string, string> {
  const at = source.indexOf(`${selector} {`);
  expect(at, `found ${selector} { ... }`).toBeGreaterThan(-1);
  const body = source.slice(source.indexOf("{", at) + 1, source.indexOf("}", at));
  const out: Record<string, string> = {};
  for (const m of body.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
    out[m[1]] = m[2].trim();
  }
  return out;
}

const hex = (h: string) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
const lum = (h: string) => {
  const [r, g, b] = hex(h).map((c) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const contrast = (a: string, b: string) => {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};

const DARK_THEMES: Record<string, string> = {
  Slate: ".dark",
  Midnight: ':root[data-theme="midnight"]',
  Graphite: ':root[data-theme="graphite"]',
  OLED: ':root[data-theme="oled"]',
  Ocean: ':root[data-theme="ocean"]',
};

const violet = () => rule(css, '.dark[data-accent="violet"]');

describe("dark violet", () => {
  test("fills with the icon's vivid violet, white on it, and keeps a lighter shade for words", () => {
    const v = violet();
    expect(v["--color-accent"]).toBe("#8655f6");
    expect(v["--color-on-accent"]).toBe("#ffffff");
    expect(v["--color-accent-text"]).toBe("#a78bfa");
    // The soft tint is made from the fill, not from the text shade.
    expect(v["--color-accent-soft"]).toMatch(/^rgb\(134 85 246 \/ 0\.\d+\)$/);
    // Hover goes deeper, so white stays readable on it.
    expect(contrast(v["--color-accent-hover"], "#ffffff")).toBeGreaterThanOrEqual(4.5);
  });

  test("white button text on the fill clears AA", () => {
    expect(contrast(violet()["--color-accent"], "#ffffff")).toBeGreaterThanOrEqual(4.5);
  });

  test.each(Object.entries(DARK_THEMES))("on %s, words clear 4.5:1 and the fill 3:1", (_name, selector) => {
    const t = rule(css, selector);
    const v = violet();
    for (const surface of [t["--color-bg"], t["--color-surface"], t["--color-surface-2"]]) {
      expect(contrast(v["--color-accent-text"], surface), `text on ${surface}`).toBeGreaterThanOrEqual(4.5);
    }
    // Rings and selected borders sit on the page and on cards.
    for (const surface of [t["--color-bg"], t["--color-surface"]]) {
      expect(contrast(v["--color-accent"], surface), `fill on ${surface}`).toBeGreaterThanOrEqual(3);
    }
  });

  test("status fills keep dark ink, not the white that sits on the violet", () => {
    const ink = violet()["--color-on-status"];
    expect(ink).toBe("#0f172a");
    const dark = rule(css, ".dark");
    for (const token of ["--color-danger", "--color-success", "--color-warning", "--color-muted"]) {
      expect(contrast(dark[token], ink), token).toBeGreaterThanOrEqual(4.5);
    }
  });
});

describe("every other theme and accent", () => {
  test("the text shade and status ink default to the accent's own, declared once", () => {
    const root = rule(css, ":root");
    expect(root["--color-accent-text"]).toBe("var(--color-accent)");
    expect(root["--color-on-status"]).toBe("var(--color-on-accent)");
  });

  test("no block but dark violet sets the text shade or status ink apart", () => {
    const setters = [...css.matchAll(/([^{}]+)\{([^}]*)\}/g)]
      .filter(([, , body]) => /--color-(accent-text|on-status)\s*:/.test(body))
      .map(([, sel]) => sel.replace(/\/\*[\s\S]*?\*\//g, "").trim())
      // @theme only turns the token into utilities; it sets no colour.
      .filter((sel) => !sel.startsWith("@theme"));
    expect(setters.sort()).toEqual([':root', '.dark[data-accent="violet"]'].sort());
  });
});

describe("the utilities", () => {
  const theme = () => rule(css, "@theme inline");

  test("text-accent paints the text shade; bg/border/ring/outline keep the fill", () => {
    expect(theme()["--text-color-accent"]).toBe("var(--color-accent-text)");
    expect(theme()["--color-accent"]).toBe("var(--color-accent)");
    expect(theme()["--text-color-accent-fill"]).toBe("var(--color-accent)");
    expect(theme()["--color-on-status"]).toBe("var(--color-on-status)");
  });

  test("XiodUI's accent-coloured words read the text shade too", () => {
    expect(bridge).toContain("--xiod-accent-text: var(--color-accent-text);");
    expect(bridge).toContain("--color-accent-foreground: var(--xiod-accent-text);");
    expect(bridge).toContain("--text-color-primary: var(--xiod-accent-text);");
  });

  test("markdown links are words", () => {
    expect(css).toMatch(/\.md-preview a\s*\{[^}]*color:\s*var\(--color-accent-text\)/);
  });

  test("nothing puts on-accent ink on a status fill", () => {
    const hits: string[] = [];
    const walk = (dir: string) => {
      for (const e of readdirSync(dir, { withFileTypes: true })) {
        const p = join(dir, e.name);
        if (e.isDirectory()) walk(p);
        else if (/\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name)) {
          readFileSync(p, "utf8")
            .split("\n")
            .forEach((line, i) => {
              if (/bg-(danger|success|warning|muted)\b[^"]*\btext-on-accent\b/.test(line)) {
                hits.push(`${relative(SRC, p)}:${i + 1}`);
              }
            });
        }
      }
    };
    walk(SRC);
    expect(hits).toEqual([]);
  });
});

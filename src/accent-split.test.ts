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
  Halloween: ':root[data-theme="halloween"]',
  Christmas: ':root[data-theme="christmas"]',
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

// The seasonal themes' own accent is what links and `text-accent` words show
// on them, so it has to read as words on every surface, and as the soft
// tint's text too. (Other themes are covered by their own palettes.)
describe("a seasonal theme's own accent", () => {
  const mix = (fg: string, bg: string, a: number) =>
    "#" +
    hex(fg)
      .map((c, i) => Math.round(c * a + hex(bg)[i] * (1 - a)).toString(16).padStart(2, "0"))
      .join("");
  const SEASONAL = { Halloween: DARK_THEMES.Halloween, Christmas: DARK_THEMES.Christmas };

  test.each(Object.entries(SEASONAL))("on %s, its words clear 4.5:1 on bg, surface, surface-2 and the soft tint", (_n, selector) => {
    const t = rule(css, selector);
    const words = t["--color-accent-text"] ?? t["--color-accent"];
    const soft = /rgb\((\d+) (\d+) (\d+) \/ ([\d.]+)\)/.exec(t["--color-accent-soft"]);
    expect(soft, "accent-soft is an rgb() with alpha").not.toBeNull();
    const tint = (surface: string) =>
      mix("#" + [1, 2, 3].map((i) => Number(soft![i]).toString(16).padStart(2, "0")).join(""), surface, Number(soft![4]));
    for (const surface of [t["--color-bg"], t["--color-surface"], t["--color-surface-2"]]) {
      expect(contrast(words, surface), `words on ${surface}`).toBeGreaterThanOrEqual(4.5);
    }
    for (const surface of [t["--color-surface"]]) {
      expect(contrast(words, tint(surface)), "words on the soft tint").toBeGreaterThanOrEqual(4.5);
    }
    expect(contrast(t["--color-on-accent"], t["--color-accent"]), "ink on the fill").toBeGreaterThanOrEqual(4.5);
    expect(contrast(t["--color-on-accent"], t["--color-accent-hover"]), "ink on hover").toBeGreaterThanOrEqual(4.5);
  });

  test("on Christmas, danger reads as words and is a different hue from the accent", () => {
    const t = rule(css, DARK_THEMES.Christmas);
    for (const surface of [t["--color-bg"], t["--color-surface"], t["--color-surface-2"]]) {
      expect(contrast(t["--color-danger"], surface), `danger on ${surface}`).toBeGreaterThanOrEqual(4.5);
    }
    const hue = (h: string) => {
      const [r, g, b] = hex(h).map((c) => c / 255);
      const mx = Math.max(r, g, b);
      const d = mx - Math.min(r, g, b);
      if (d === 0) return 0;
      const x = mx === r ? ((g - b) / d) % 6 : mx === g ? (b - r) / d + 2 : (r - g) / d + 4;
      return (x * 60 + 360) % 360;
    };
    const gap = Math.abs(hue(t["--color-danger"]) - hue(t["--color-accent"]));
    expect(Math.min(gap, 360 - gap)).toBeGreaterThanOrEqual(15);
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

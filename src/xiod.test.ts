/**
 * XiodUI gate - the owner's conditions for taking the library (spec:
 * docs/superpowers/specs/2026-09-23-xiod-ui-refresh-design.md), checked on
 * every run so a routine `npm install` cannot quietly change them:
 * - xiod-ui is pinned to exactly the version that was read and vetted, and
 *   the lockfile still holds the vetted versions of what it pulls in;
 * - its PolyForm Perimeter notices ship with the app;
 * - it is drawn in the app's own colours: every XiodUI colour name the
 *   scanned parts use is mapped onto an app token in src/xiod-theme.css;
 * - only the vetted parts are imported (theme-provider and resizable, the
 *   ones that touch localStorage, never are).
 */
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const SRC = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(SRC, "..");
const read = (...parts: string[]) => readFileSync(resolve(ROOT, ...parts), "utf8");

/** The XiodUI parts the app may import - each one read during vetting. */
const VETTED_PARTS = ["checkbox", "kbd", "textarea", "toast", "command", "calendar"];

const bridge = () => read("src", "xiod-theme.css");
const indexCss = () => read("src", "index.css");
/** Component files the bridge tells Tailwind to scan. */
const scanned = () =>
  [...bridge().matchAll(/@source "\.\.\/node_modules\/xiod-ui\/dist\/components\/([a-z-]+)\.js";/g)].map((m) => m[1]);
/** XiodUI's semantic colour names, from its own stylesheet's @theme. */
const xiodColourNames = () => [...read("node_modules", "xiod-ui", "dist", "styles.css").matchAll(/--color-([a-z-]+):/g)].map((m) => m[1]);

describe("pin", () => {
  test("xiod-ui is pinned exactly, and React sits on 19.3", () => {
    const pkg = JSON.parse(read("package.json"));
    expect(pkg.dependencies["xiod-ui"]).toBe("1.0.3");
    expect(pkg.dependencies.react).toBe("~19.3.0");
    expect(pkg.dependencies["react-dom"]).toBe("~19.3.0");
  });

  test("the lockfile holds the versions that were vetted", () => {
    const lock = JSON.parse(read("package-lock.json"));
    const v = (name: string) => lock.packages[`node_modules/${name}`]?.version;
    expect({
      "xiod-ui": v("xiod-ui"),
      "xiod-icons": v("xiod-icons"),
      "@base-ui/react": v("@base-ui/react"),
      "@base-ui/utils": v("@base-ui/utils"),
      cn: v("cn"),
      "class-variance-authority": v("class-variance-authority"),
    }).toEqual({
      "xiod-ui": "1.0.3",
      "xiod-icons": "1.1.1",
      "@base-ui/react": "1.8.0",
      "@base-ui/utils": "0.4.0",
      cn: "0.3.3",
      "class-variance-authority": "0.7.1",
    });
  });
});

describe("notices", () => {
  test.each(["xiod-ui", "xiod-icons"])("%s: its version, its terms and every Required Notice line ship", (name) => {
    const notices = read("public", "THIRD-PARTY-NOTICES.txt");
    const version = JSON.parse(read("node_modules", name, "package.json")).version;
    expect(notices).toContain(`${name} ${version}`);
    expect(notices).toContain("https://polyformproject.org/licenses/perimeter/1.0.1");
    const required = read("node_modules", name, "LICENSE")
      .split(/\r?\n/)
      .filter((l) => l.startsWith("Required Notice:"));
    expect(required.length).toBeGreaterThan(0);
    for (const line of required) expect(notices).toContain(line.trim());
  });
});

describe("theme bridge", () => {
  test("index.css brings the bridge in, and XiodUI's own stylesheet stays out", () => {
    expect(indexCss()).toContain('@import "./xiod-theme.css";');
    for (const css of [indexCss(), bridge()]) expect(css).not.toMatch(/xiod-ui\/(dist\/)?(styles|themes)/);
  });

  test("the bridge names no colour of its own - tokens only", () => {
    expect(bridge()).not.toMatch(/#[0-9a-fA-F]{3,8}\b|\brgba?\(|\bhsla?\(|\boklch\(/);
  });

  test("every scanned XiodUI file exists", () => {
    expect(scanned().length).toBeGreaterThan(0);
    for (const f of scanned()) {
      expect(existsSync(resolve(ROOT, "node_modules", "xiod-ui", "dist", "components", `${f}.js`)), f).toBe(true);
    }
  });

  test("every XiodUI colour the scanned parts use is mapped onto an app token", () => {
    const defined = new Set([...(bridge() + indexCss()).matchAll(/--color-([a-z0-9-]+):/g)].map((m) => m[1]));
    const missing: string[] = [];
    for (const f of scanned()) {
      const js = read("node_modules", "xiod-ui", "dist", "components", `${f}.js`);
      for (const name of xiodColourNames()) {
        const used = new RegExp(
          `\\b(?:bg|text|border|ring|ring-offset|fill|stroke|shadow|inset-shadow|outline|divide|from|to|via)-${name}(?:/\\d+)?(?![\\w-])`,
        ).test(js);
        if (used && !defined.has(name)) missing.push(`${f}.js: ${name}`);
      }
    }
    expect(missing).toEqual([]);
  });

  test("raw XiodUI variables read inside arbitrary values are defined", () => {
    const names = new Set(xiodColourNames());
    const missing = new Set<string>();
    for (const f of scanned()) {
      const js = read("node_modules", "xiod-ui", "dist", "components", `${f}.js`);
      for (const m of js.matchAll(/var\(--([a-z-]+)\)/g)) {
        if (names.has(m[1]) && !bridge().includes(`  --${m[1]}:`)) missing.add(`${f}.js: --${m[1]}`);
      }
    }
    expect([...missing]).toEqual([]);
  });

  test("inside XiodUI's panels, accent and muted mean its quiet highlight and wash", () => {
    const block = bridge().match(/\[data-slot="calendar"\],[^{]*\{([^}]*)\}/)?.[1] ?? "";
    expect(block).toContain("--color-accent: var(--color-accent-soft);");
    expect(block).toContain("--color-muted: var(--color-surface-2);");
  });

  test("every XiodUI part stands still under reduced motion", () => {
    const css = bridge();
    const guard = css.slice(css.indexOf("@media (prefers-reduced-motion: reduce)"));
    for (const slot of [
      '[data-slot^="checkbox"]',
      '[data-slot^="textarea"]',
      '[data-slot^="toast"]',
      '[data-slot^="command"]',
      '[data-slot="calendar"]',
    ]) {
      expect(guard).toContain(slot);
    }
    expect(guard).toContain("transition: none !important;");
    expect(guard).toContain("animation: none !important;");
  });
});

describe("imports", () => {
  test("only vetted XiodUI parts are imported, and each is scanned for its classes", () => {
    const imported: { file: string; part: string }[] = [];
    const walk = (dir: string) => {
      for (const e of readdirSync(dir, { withFileTypes: true })) {
        const p = join(dir, e.name);
        if (e.isDirectory()) {
          walk(p);
          continue;
        }
        if (!/\.(tsx?|jsx?)$/.test(e.name)) continue;
        for (const m of readFileSync(p, "utf8").matchAll(/from "xiod-ui\/([a-z/-]+)"/g)) {
          imported.push({ file: relative(SRC, p).replace(/\\/g, "/"), part: m[1] });
        }
      }
    };
    walk(SRC);
    expect(imported.filter((i) => !VETTED_PARTS.includes(i.part))).toEqual([]);
    expect(imported.filter((i) => !scanned().includes(i.part))).toEqual([]);
  });
});

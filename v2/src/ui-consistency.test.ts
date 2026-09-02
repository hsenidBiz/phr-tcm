/**
 * UI consistency gate - source-level rules that keep every screen on the
 * token system so the app cannot drift away from its one look.
 *
 * The rules encode decisions already made:
 * - colors come from theme tokens (text-text/muted/faint, bg-surface,
 *   border-border, accent/success/warning/danger), never from Tailwind's
 *   default palette - that is exactly how static styles and
 *   refresh_theme() drift apart (see the Slate-reversion incident);
 * - raw hex and white/black are allowed only where they are deliberate
 *   (modal backdrops, avatar initials over hashed colors, DevOps-defined
 *   type/state colors) - the allowlists below name every one;
 * - no text below 10px: smaller is unreadable at normal DPI.
 *
 * A failure here is not a broken feature - it is a new file quietly
 * introducing a second design language. Fix it by using tokens, or, when
 * the color is genuinely fixed (a brand mark, an over-photo label), add
 * the file to the allowlist WITH a reason.
 */
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

// import.meta.url, not __dirname: this file is ESM under vitest.
const SRC = dirname(fileURLToPath(import.meta.url));

function sources(): { file: string; text: string }[] {
  const out: { file: string; text: string }[] = [];
  const walk = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name);
      if (e.isDirectory()) {
        if (e.name === "dev") continue; // dev tools never ship
        walk(p);
        continue;
      }
      if (!/\.(tsx|ts)$/.test(e.name)) continue;
      if (/\.test\.(tsx|ts)$/.test(e.name)) continue;
      if (e.name === "bindings.ts") continue; // generated
      out.push({ file: relative(SRC, p).replace(/\\/g, "/"), text: readFileSync(p, "utf-8") });
    }
  };
  walk(SRC);
  return out;
}

const files = sources();

function violations(re: RegExp, allow: Set<string>): string[] {
  const hits: string[] = [];
  for (const { file, text } of files) {
    if (allow.has(file)) continue;
    text.split("\n").forEach((line, i) => {
      if (line.trimStart().startsWith("//") || line.trimStart().startsWith("*")) return;
      const m = line.match(re);
      if (m) hits.push(`${file}:${i + 1}  ${m[0]}`);
    });
  }
  return hits;
}

describe("color system", () => {
  test("no Tailwind default-palette colors - theme tokens only", () => {
    const re =
      /(?:text|bg|border|ring|fill|stroke|from|to|via)-(?:gray|slate|zinc|neutral|stone|red|green|blue|yellow|amber|violet|purple|rose|orange|teal|cyan|sky|indigo|lime|emerald|fuchsia|pink)-[0-9]{2,3}/;
    expect(violations(re, new Set())).toEqual([]);
  });

  test("raw hex colors only where deliberately fixed", () => {
    const allow = new Set([
      "lib/theme.ts", // the token definitions themselves
      "lib/astryxTheme.ts", // Astryx defineTheme fallbacks when live tokens absent
      "lib/pbiGlow.ts", // glow effect color math
      "components/CommentsPanel.tsx", // avatar initials: hashed color palette
      "screens/Settings.tsx", // accent swatches + system-theme preview
      "screens/PrPanel.tsx", // DevOps work-item type colors (match the board)
      "screens/WorkBoard.tsx", // DevOps state/type colors from the API
      "components/WorkItemDrawer.tsx", // DevOps state colors from the API
      "screens/SignIn.tsx", // flask/branding animation
      "tour/UiTour.tsx", // spotlight overlay math
    ]);
    expect(violations(/#[0-9a-fA-F]{6}\b/, allow)).toEqual([]);
  });

  test("white/black only for backdrops and over-fixed-color content", () => {
    const allow = new Set([
      "components/ui/modal.tsx", // backdrop
      "components/CommandPalette.tsx", // backdrop
      "components/WorkItemDrawer.tsx", // backdrop
      "screens/ViewCases/CommentModal.tsx", // backdrop
      "components/CommentsPanel.tsx", // initials over hashed avatar color
      "components/TitleBar.tsx", // close button over hover-danger
      "screens/Settings.tsx", // checkmark over accent swatch
      "tour/UiTour.tsx", // spotlight overlay
      "screens/SignIn.tsx", // branding
    ]);
    expect(violations(/(?:text|bg)-(?:white|black)(?![-\w])/, allow)).toEqual([]);
  });
});

describe("readability", () => {
  test("no text below 10px", () => {
    const hits: string[] = [];
    for (const { file, text } of files) {
      for (const m of text.matchAll(/text-\[(\d+)px\]/g)) {
        if (Number(m[1]) < 10) hits.push(`${file}: text-[${m[1]}px]`);
      }
    }
    expect(hits).toEqual([]);
  });

  test("icon-only interactive elements carry an accessible name", () => {
    // Source heuristic: a <button> whose JSX body is ONLY an icon component
    // must have aria-label or title. Rendered-level enforcement lives in
    // a11y.test.tsx; this catches files with no test coverage at all.
    const hits: string[] = [];
    for (const { file, text } of files) {
      const re = /<button\s[^>]*>\s*<[A-Z]\w+\s+size=\{?\d+\}?[^>]*\/>\s*<\/button>/gs;
      for (const m of text.matchAll(re)) {
        if (!/aria-label|title=/.test(m[0])) hits.push(`${file}: ${m[0].slice(0, 60)}...`);
      }
    }
    expect(hits).toEqual([]);
  });
});

describe("button icons", () => {
  // An icon beside a label makes a button quicker to FIND. It must not
  // change what the button IS called: the label already names the action,
  // so an announced icon would only repeat it, and every test that finds a
  // button by its accessible name would start matching something else.
  test("every button icon is aria-hidden", () => {
    const hits: string[] = [];
    for (const { file, text } of files) {
      text.split("\n").forEach((line, i) => {
        const m = line.match(/<Icon[A-Za-z]+\b(?![^/>]*aria-hidden)[^>]*\/>/);
        if (m) hits.push(`${file}:${i + 1}  ${m[0]}`);
      });
    }
    expect(hits).toEqual([]);
  });

  // One intent, one icon. Importing a lucide icon straight into a screen is
  // how the same action ends up with two different pictures on two tabs -
  // the vocabulary in lib/actionIcons.ts is the single place to add one.
  test("action icons come from the shared vocabulary, not straight from lucide", () => {
    const vocabulary = new Set(
      (files.find((f) => f.file === "lib/actionIcons.ts")?.text ?? "")
        .split("\n")
        .flatMap((l) => l.match(/as (Icon[A-Za-z]+),/)?.slice(1) ?? []),
    );
    expect(vocabulary.size).toBeGreaterThan(20);

    const hits: string[] = [];
    for (const { file, text } of files) {
      if (file === "lib/actionIcons.ts") continue;
      for (const m of text.matchAll(/<(Icon[A-Za-z]+)\b/g)) {
        if (!vocabulary.has(m[1])) hits.push(`${file}  ${m[1]}`);
      }
    }
    expect(hits).toEqual([]);
  });

  // Size is the Button's job ([&_svg]:size-*), so a call site that passes
  // its own is a 14px icon next to a 16px one on the next screen.
  test("no hand-sized icons inside buttons", () => {
    const hits: string[] = [];
    for (const { file, text } of files) {
      for (const m of text.matchAll(/<Icon[A-Za-z]+[^>]*\bsize=\{?\d/g)) {
        hits.push(`${file}  ${m[0]}`);
      }
    }
    expect(hits).toEqual([]);
  });
});

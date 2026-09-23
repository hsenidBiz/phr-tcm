# XiodUI Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Six controls (toasts, checkbox, text area, command palette list, keyboard-key hints, date picker calendar) take their look and behaviour from XiodUI 1.0.3, each behind the app's own wrapper, drawn in the app's own theme tokens.

**Architecture:** `xiod-ui` is installed pinned to exactly 1.0.3 and vetted. Its stylesheet is NOT imported (it redefines colour names the app already owns); instead `src/xiod-theme.css` tells Tailwind to scan only the XiodUI parts the app uses and maps XiodUI's semantic colour names onto the app's tokens, so every theme and accent preset reaches them. Each control is swapped inside its existing wrapper in `src/components/ui/` (names and props unchanged); toasts go through one new local module `src/lib/toast.ts` that forwards to XiodUI's toast manager, with one host component `src/components/ui/toaster.tsx`.

**Tech Stack:** React 19.3, TypeScript 5.8, Tailwind CSS 4.3 (`@tailwindcss/vite`), `xiod-ui` 1.0.3 (Base UI 1.8.0 + `cn` + class-variance-authority underneath), Vitest 4 + Testing Library (jsdom).

**Spec:** `docs/superpowers/specs/2026-09-23-xiod-ui-refresh-design.md`

## Global Constraints

- Colours come from theme tokens only (`text-text`, `bg-surface`, `border-border`, `text-accent`...): no hex, no rgb/hsl/oklch, no Tailwind palette classes - in TSX and in `src/xiod-theme.css`.
- Icons come from `src/lib/actionIcons.ts` (a new one is added there, named for what the control DOES), rendered `aria-hidden`, never with a `size=` prop.
- `src/ui-consistency.test.ts` and `src/a11y.test.tsx` are never weakened. A test is updated only where markup legitimately changed (say so in the step), never loosened.
- `xiod-ui` is pinned to exactly `"1.0.3"` (no caret). `react` and `react-dom` are `"~19.3.0"` (19.3.x). `@types/react` / `@types/react-dom` move to `"^19.3.0"` (a bump, not a new package).
- No other new direct packages unless the task says so. (`xiod-ui` brings `xiod-icons`, `@base-ui/react`, `@base-ui/utils`, `cn`, `@floating-ui/*`, `reselect`, `use-sync-external-store` transitively; Task 1 vets them.)
- Never import `xiod-ui/styles`, `xiod-ui/themes/*`, `xiod-ui/theme-provider` or `xiod-ui/resizable`. The app imports only the vetted parts: `checkbox`, `kbd`, `textarea`, `toast`, `command`, `calendar` (`src/xiod.test.ts` enforces this).
- No Rust changes; `src/bindings.ts` unchanged.
- Frontend commands, run from the repo root, ONE at a time, waiting for each: `npx vitest run <paths> --exclude "**/.claude/**"`, `npx tsc --noEmit`, `npm run build`. The machine is shared: never two suites at once.
- Search with the Grep/Read tools, not Bash grep (it hangs here).
- Edit/Write tools for source; keep the files' CRLF line endings. `git add` files by path, never `git add -A` / `.`.
- Commit with a Bash heredoc (`git commit -F - <<'EOF' ... EOF`), never PowerShell message flags; end the message with a `Co-Authored-By:` trailer naming the model actually committing (the steps below show `Claude Opus 5.5`; substitute yours). Confirm with `git log -1`.
- No em dashes in user-facing text (UI strings, notices, commit subjects); use a hyphen.
- Not a release: no version bump, no `changelog.ts` entry.
- `src/App.test.tsx` is slow and has a documented load-induced flake: one failure that passes on a re-run is that; two different failures are not.

## Review Focus

- A Checkbox inside a wrapping `<label>` (PowerRenameDialog, multiselect, BulkEditDialog): one click on the label text OR on the box toggles exactly once, and the box is named by the label. Tests in Task 3.
- A Textarea whose call site sets `text-xs` (BugDialog, RunPane, RecipeEditor, ScriptEditor) in a window wider than 640px: the text stays xs, because XiodUI's own `sm:text-sm` would otherwise win. Test in Task 4.
- A toast's action (Undo after Discard changes) runs once and takes only its own toast away; any other toast stays. Test in Task 2.
- DateField in the work-item drawer: with the calendar open (it now holds focus), Escape closes only the calendar and focus returns to the field; it must not reach the drawer's window-level Escape, which would close the drawer and its draft. Test in Task 6.
- Command palette with a query that matches nothing: it says "No results." and Enter runs nothing. Test in Task 5.

Note for reviewers: Base UI renders each toast as `role="dialog"` named by its message. Every current `getByRole("dialog", ...)` in the suite passes a name, so none becomes ambiguous; an unnamed one added later would.

## Vetting record (spec §4, read before planning; Task 1 re-checks the installed tree)

Read from `npm pack` tarballs of `xiod-ui@1.0.3`, `xiod-icons@1.1.1`, `cn@0.3.3`, `@base-ui/react@1.8.0`, `@base-ui/utils@0.4.0`, `@floating-ui/react-dom@2.1.9`, `@floating-ui/dom@1.8.0`, `@floating-ui/utils@0.2.12`, `use-sync-external-store@1.7.0` (class-variance-authority 0.7.1 is already in the app).

- **Install scripts:** none. `xiod-ui` and `xiod-icons` have only `prepack` / `prepublishOnly` (run by their publisher, never on a consumer's install); `cn`, Base UI, floating-ui, use-sync-external-store have no install/preinstall/postinstall/prepare.
- **Network:** no `fetch`, `XMLHttpRequest`, `WebSocket`, `sendBeacon`, `postMessage` anywhere in the runtime code.
- **Dynamic code:** no `eval` / `new Function`. The only `import()` is in `cn`'s build CLI (`cn/dist/build.js`, `cn/bin/cn.mjs`), which the `cn` runtime entry that XiodUI imports never loads.
- **Storage / clipboard / DOM injection:** `localStorage` only in `xiod-ui/dist/components/theme-provider.js` and `resizable.js`; clipboard only in `copy-to-clipboard.js`, `input-sensitive.js` and `hooks/use-copy-to-clipboard.js`; `innerHTML` only in `resizable.js`. None is among the parts this app imports (checkbox, kbd, textarea, toast, command, calendar and what they import: button, autocomplete, input, scroll-area, scroll-bar).
- **IPC:** no `__TAURI` reference anywhere.
- **Licences:** `xiod-ui` and `xiod-icons` are both PolyForm Perimeter 1.0.1, each with `Required Notice: Copyright 2026 ImKKingshuk (https://github.com/ImKKingshuk)`. `cn`, Base UI, floating-ui: MIT.
- **Verdict:** pass, on condition that the app imports only the vetted parts (enforced by `src/xiod.test.ts`) and the lockfile keeps the vetted versions (also enforced there).

## Scope notes

- The app renders no `<kbd>`; its one hint component is Astryx's `Kbd` in the command palette. That is what the shared `Kbd` wrapper replaces. Plain-text hints ("Ctrl+click to toggle", "Ctrl+K for commands") stay text.
- Settings has no About area, so the notices file ships in the bundle only (`public/` is copied into `dist/` by Vite).
- Sonner options in use across the app, inventoried: `description`, `duration`, `action: { label, onClick }`, and `toast.dismiss()` with no id. No call site uses `id`, `promise`, `loading`, `custom`, `cancel`, `onDismiss`/`onAutoClose`, `important` or per-toast `position`. `src/lib/toast.ts` covers exactly the used set plus the fixed interface.

> Deviation from spec: the Date picker takes XiodUI's `Calendar` inside the app's own inline DateField panel, not XiodUI's `DatePicker` / `Popover`. `DatePicker` gives its trigger no accessible name (DateField's `ariaLabel` has nowhere to go) and has no Clear; and its `Popover` portals to `<body>`, outside the work-item drawer's `useFocusTrap`, which would pull keyboard focus back out of the calendar on Tab.

## File map

| File | Task | Responsibility |
|---|---|---|
| `package.json`, `package-lock.json` | 1, 2, 5, 6 | pin xiod-ui, React 19.3; later remove sonner, cmdk, react-day-picker |
| `src/xiod-theme.css` (new) | 1 | Tailwind sources for the used XiodUI parts; XiodUI colour names -> app tokens; dark variant; reduced motion |
| `src/index.css` | 1, 2, 3 | import the bridge; drop sonner's override block; drop `.t-check` rules |
| `public/THIRD-PARTY-NOTICES.txt` (new) | 1 | PolyForm Perimeter notices for xiod-ui and xiod-icons |
| `src/xiod.test.ts` (new) | 1 | pin, vetted versions, notices, bridge coverage, import allowlist |
| `src/lib/toast.ts` (new) | 2 | the app's `toast` API, forwarding to XiodUI's toast manager |
| `src/components/ui/toaster.tsx` (new) | 2 | the one toast host per window |
| `src/lib/toast.test.tsx` (new) | 2 | toast module + host behaviour |
| `src/components/HoverDismissToaster.tsx` (+ test) | 2 | runner's host whose toasts leave on hover |
| `src/components/ui/checkbox.tsx` (+ tests, `motion.test.tsx`) | 3 | Checkbox wrapper over `xiod-ui/checkbox` |
| `src/components/ui/input.tsx`, `input.test.tsx` (new) | 4 | Textarea wrapper over `xiod-ui/textarea` |
| `src/components/ui/kbd.tsx`, `kbd.test.tsx` (new) | 5 | shared key-hint wrapper over `xiod-ui/kbd` |
| `src/components/CommandPalette.tsx` (+ test) | 5 | palette list over `xiod-ui/command` |
| `src/components/ui/calendar.tsx`, `datefield.tsx`, `datefield.test.tsx` (new) | 6 | date picker over `xiod-ui/calendar` |
| `src/lib/actionIcons.ts` | 6 | `IconPickDate` |
| `README.md` | 7 | architecture note |

---

### Task 1: XiodUI installed, vetted, themed from the app's tokens, with its notices

**Files:**
- Create: `src/xiod.test.ts`
- Create: `src/xiod-theme.css`
- Create: `public/THIRD-PARTY-NOTICES.txt`
- Modify: `package.json` (dependencies, devDependencies), `package-lock.json` (via `npm install`)
- Modify: `src/index.css:16` (one `@import` after the Fira Code imports)

**Interfaces:**
- Consumes: nothing.
- Produces: XiodUI importable as `xiod-ui/<part>`; Tailwind utilities `bg-background`, `bg-popover`, `text-popover-foreground`, `bg-primary`, `text-primary-foreground`, `text-muted-foreground`, `text-accent-foreground`, `border-input`, `ring-ring`, `text-destructive`, `text-info`, ... resolving to app tokens; `dark:` = the app's `.dark` class; later tasks add their part to `VETTED_PARTS`-listed imports only (`checkbox`, `kbd`, `textarea`, `toast`, `command`, `calendar`).

- [ ] **Step 1: Write the failing test**

Create `src/xiod.test.ts`:

```ts
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
    for (const css of [indexCss(), bridge()]) expect(css).not.toMatch(/xiod-ui\/(styles|themes)/);
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
```

- [ ] **Step 2: Run it to verify it fails**

Run: `npx vitest run src/xiod.test.ts --exclude "**/.claude/**"`
Expected: FAIL - "pin" fails (`expected undefined to be '1.0.3'`), "notices" and "theme bridge" fail with `ENOENT` (no notices file, no `src/xiod-theme.css`, no `node_modules/xiod-ui`).

- [ ] **Step 3: Pin the packages**

Edit `package.json`:
- in `dependencies`, replace `"react": "^19.1.0",` with `"react": "~19.3.0",` and `"react-dom": "^19.1.0",` with `"react-dom": "~19.3.0",`;
- in `dependencies`, after `"turndown-plugin-gfm": "^1.0.2"` add a comma and the line `"xiod-ui": "1.0.3"` (last entry, no trailing comma);
- in `devDependencies`, replace `"@types/react": "^19.1.8",` with `"@types/react": "^19.3.0",` and `"@types/react-dom": "^19.1.6",` with `"@types/react-dom": "^19.3.0",`.

Leave `allowScripts` as it is: it already blocks any dependency install script not listed, which is the behaviour vetting wants.

- [ ] **Step 4: Install**

Run: `npm install`
Expected: completes; no new install script is run or requested. If npm stops on a peer-dependency conflict (ERESOLVE), stop and report the conflict; do not use `--force` or `--legacy-peer-deps`.

- [ ] **Step 5: Re-vet the installed tree (spec §4)**

With the Grep tool (not Bash), confirm each expectation; stop and report if any differs from the "Vetting record" above:
1. `Read` each of `node_modules/xiod-ui/package.json`, `node_modules/xiod-icons/package.json`, `node_modules/cn/package.json`, `node_modules/@base-ui/react/package.json`, `node_modules/@base-ui/utils/package.json`, `node_modules/reselect/package.json`, `node_modules/@floating-ui/core/package.json`, `node_modules/@floating-ui/dom/package.json`, `node_modules/@floating-ui/react-dom/package.json`, `node_modules/@floating-ui/utils/package.json`, `node_modules/use-sync-external-store/package.json`: no `preinstall`, `install`, `postinstall` or `prepare` script.
2. Grep pattern `\beval\(|new Function\(|\bfetch\(|XMLHttpRequest|WebSocket|sendBeacon|postMessage|__TAURI` with `output_mode: files_with_matches` over each of `node_modules/xiod-ui/dist`, `node_modules/xiod-icons/dist`, `node_modules/cn/dist`, `node_modules/@base-ui`, `node_modules/@floating-ui`, `node_modules/reselect/dist`, `node_modules/use-sync-external-store`, `node_modules/class-variance-authority/dist`: no files.
3. Grep pattern `import\(` over `node_modules/cn` (glob `*.{js,mjs,cjs}`): only `dist/build.js`, `dist/build.cjs`, `bin/cn.mjs`. Grep `from "./build` over `node_modules/cn/dist/index.js`: no match (the runtime entry never loads the CLI).
4. Grep pattern `localStorage|sessionStorage|indexedDB|document\.cookie|clipboard|innerHTML` over `node_modules/xiod-ui/dist` (glob `*.js`): only `components/theme-provider.js`, `components/resizable.js`, `components/copy-to-clipboard.js`, `components/input-sensitive.js`, `hooks/use-copy-to-clipboard.js`.

Keep the result for the commit message (Step 13).

- [ ] **Step 6: Write the theme bridge**

Create `src/xiod-theme.css`:

```css
/* XiodUI (xiod-ui, pinned 1.0.3) drawn in the app's own colours.

   XiodUI's stylesheet (`xiod-ui/styles`) is deliberately NOT imported. Its
   @theme block redefines --color-border, --color-muted, --color-accent,
   --color-success and --color-warning - names this app already owns, and
   in two cases uses differently (the app's `text-muted` is grey TEXT,
   XiodUI's `bg-muted` a faint WASH) - and its base layer restyles every
   element's border and outline. What it would have provided is rebuilt
   here from the app's tokens, so every theme and accent preset in
   index.css reaches the XiodUI parts with no second palette to keep in
   step.

   A new XiodUI part means a new @source line below and an entry in
   VETTED_PARTS (src/xiod.test.ts), which fails until every colour the part
   uses is mapped here. */

/* 1. Tailwind generates the utilities used inside the XiodUI parts the app
      renders - these files only, not all 89 components'. */
@source "../node_modules/xiod-ui/dist/components/checkbox.js";
@source "../node_modules/xiod-ui/dist/components/kbd.js";
@source "../node_modules/xiod-ui/dist/components/textarea.js";
@source "../node_modules/xiod-ui/dist/components/toast.js";
@source "../node_modules/xiod-ui/dist/components/button.js";
@source "../node_modules/xiod-ui/dist/components/command.js";
@source "../node_modules/xiod-ui/dist/components/autocomplete.js";
@source "../node_modules/xiod-ui/dist/components/input.js";
@source "../node_modules/xiod-ui/dist/components/scroll-area.js";
@source "../node_modules/xiod-ui/dist/components/scroll-bar.js";
@source "../node_modules/xiod-ui/dist/components/calendar.js";

/* 2. `dark:` follows the app's .dark class (lib/theme.ts), not the OS.
      Nothing in src/ uses `dark:` itself; this is for XiodUI's classes. */
@custom-variant dark (&:where(.dark, .dark *));

/* 3. Captured once on <html>, where every theme and accent preset lands.
      Custom properties inherit their RESOLVED value, so the overrides in 5.
      cannot feed back into these. */
:root {
  --xiod-accent: var(--color-accent);
  --xiod-muted-ink: var(--color-muted);
  /* Raw names XiodUI reads inside arbitrary values (shadows, insets). */
  --border: var(--color-border);
  --input: var(--color-border);
  --primary-foreground: var(--color-on-accent);
}

/* 4. XiodUI's semantic colour names, as app tokens. The five the app
      already owns (border, accent, muted, success, warning) are not
      redefined here; 5. handles the two whose meaning differs. */
@theme inline {
  --color-background: var(--color-surface);
  --color-foreground: var(--color-text);
  --color-card: var(--color-surface);
  --color-card-foreground: var(--color-text);
  --color-popover: var(--color-surface);
  --color-popover-foreground: var(--color-text);
  --color-primary: var(--xiod-accent);
  --color-primary-foreground: var(--color-on-accent);
  --color-secondary: var(--color-surface-2);
  --color-secondary-foreground: var(--color-text);
  --color-muted-foreground: var(--xiod-muted-ink);
  --color-accent-foreground: var(--xiod-accent);
  --color-destructive: var(--color-danger);
  --color-destructive-foreground: var(--color-danger);
  --color-info: var(--xiod-accent);
  --color-info-foreground: var(--xiod-accent);
  --color-success-foreground: var(--color-success);
  --color-warning-foreground: var(--color-warning);
  --color-input: var(--color-border);
  --color-ring: var(--xiod-accent);
}

/* 5. Inside XiodUI's own panels, "accent" is its quiet highlight (a hovered
      day, the highlighted command) and "muted" a faint wash: the app's
      accent-soft and surface-2. The brand accent and grey text stay
      reachable there as primary, accent-foreground and muted-foreground. */
[data-slot="calendar"],
[data-slot="command-dialog-popup"],
[data-slot="toast-viewport"] {
  --color-accent: var(--color-accent-soft);
  --color-muted: var(--color-surface-2);
}

/* 6. XiodUI animates in plain CSS transitions. The app's rule (index.css,
      "Motion") is that none of it plays when the OS asks for less. */
@media (prefers-reduced-motion: reduce) {
  [data-slot^="checkbox"],
  [data-slot^="checkbox"] *,
  [data-slot^="textarea"],
  [data-slot^="textarea"] *,
  [data-slot^="toast"],
  [data-slot^="toast"] *,
  [data-slot^="command"],
  [data-slot^="command"] *,
  [data-slot="calendar"],
  [data-slot="calendar"] * {
    transition: none !important;
    animation: none !important;
  }
}
```

- [ ] **Step 7: Import the bridge**

Edit `src/index.css`: replace

```css
@import "@fontsource/fira-code/600.css";
```

with

```css
@import "@fontsource/fira-code/600.css";
/* XiodUI's parts in the app's own tokens - see the file. */
@import "./xiod-theme.css";
```

- [ ] **Step 8: Write the notices**

Create `public/THIRD-PARTY-NOTICES.txt`:

```text
Third-party notices
===================

This app includes the software below. Each part is used under the terms
named with it.

----------------------------------------------------------------------------
xiod-ui 1.0.3 (https://ui.xiod.dev)

Required Notice: Copyright 2026 ImKKingshuk (https://github.com/ImKKingshuk)

Terms: PolyForm Perimeter License 1.0.1
https://polyformproject.org/licenses/perimeter/1.0.1

----------------------------------------------------------------------------
xiod-icons 1.1.1 (https://icons.xiod.dev)

Required Notice: Copyright 2026 ImKKingshuk (https://github.com/ImKKingshuk)

Terms: PolyForm Perimeter License 1.0.1
https://polyformproject.org/licenses/perimeter/1.0.1
```

- [ ] **Step 9: Run the gate tests to verify they pass**

Run: `npx vitest run src/xiod.test.ts src/ui-consistency.test.ts --exclude "**/.claude/**"`
Expected: PASS (all). If "every XiodUI colour ... is mapped" lists a name, add its mapping to block 4 of the bridge (an app token, never a literal colour) and re-run.

- [ ] **Step 10: Full suite on React 19.3**

Run: `npx vitest run --exclude "**/.claude/**"`
Expected: PASS. Nothing in `src/` changed except CSS, so a failure here is the React 19.3 bump: investigate it (superpowers:systematic-debugging) before going on.

- [ ] **Step 11: Typecheck**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 12: Production build, and the bridge reached it**

Run: `npm run build`
Expected: succeeds (Vite may warn that `"use client"` directives were ignored; that is expected for XiodUI's files). Then with Glob confirm `dist/THIRD-PARTY-NOTICES.txt` exists, and with Grep over `dist/assets` (glob `*.css`) confirm both patterns `data-slot=\W?calendar` and `--xiod-accent` occur (the first proves the bridge's selectors reached the build, the second its token capture).

- [ ] **Step 13: Commit**

```bash
git add package.json package-lock.json src/xiod-theme.css src/index.css public/THIRD-PARTY-NOTICES.txt src/xiod.test.ts
git commit -F - <<'EOF'
feat(v2): XiodUI 1.0.3 comes in, drawn in the app's own colours, with its notices

xiod-ui is pinned to exactly 1.0.3 and React moves to 19.3 (its peer).
Its stylesheet is not imported; src/xiod-theme.css maps its colour names
onto the app's tokens, so every theme and accent preset reaches it.
public/THIRD-PARTY-NOTICES.txt carries its PolyForm Perimeter notices.

Vetting (installed tree): no install scripts; no network, eval or IPC in
xiod-ui, xiod-icons, cn, Base UI, floating-ui, reselect or
use-sync-external-store; storage, clipboard and innerHTML only in XiodUI
parts the app does not import. src/xiod.test.ts holds the pin, the vetted
lockfile versions, the notices and the import allowlist.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 2: Toasts through one local module, drawn by XiodUI

**Files:**
- Create: `src/lib/toast.ts`, `src/components/ui/toaster.tsx`, `src/lib/toast.test.tsx`
- Modify (rewrite): `src/components/HoverDismissToaster.tsx`, `src/components/HoverDismissToaster.test.tsx`
- Modify: `src/App.tsx:14`, `src/App.tsx:70`, `src/App.tsx:908-914`
- Modify: `src/screens/RunnerWindow.tsx:7`, `:24`, `:886`
- Modify: the 36 other source files and 18 test files in the codemod tables (Step 7, Step 8)
- Modify: `src/index.css:820-860` (sonner's action-button override goes)
- Modify: `package.json`, `package-lock.json` (sonner removed)

**Interfaces:**
- Consumes: `toastManager`, `ToastProvider` from `xiod-ui/toast` (Task 1 installed it).
- Produces (FIXED - other plans rely on it):
  - `src/lib/toast.ts`:
    ```ts
    export type ToastOptions = {
      description?: ReactNode;
      duration?: number; // ms; left out = the host's default (4000)
      action?: { label: ReactNode; onClick: () => void }; // clicking also dismisses the toast
    };
    export const toast: ((message: ReactNode, opts?: ToastOptions) => string) & {
      success(message: ReactNode, opts?: ToastOptions): string;
      error(message: ReactNode, opts?: ToastOptions): string;
      info(message: ReactNode, opts?: ToastOptions): string;
      warning(message: ReactNode, opts?: ToastOptions): string;
      dismiss(id?: string): void; // no id = every toast
    };
    ```
    It exports nothing else (test files mock it as `{ toast: {...} }`).
  - `src/components/ui/toaster.tsx`: `export function Toaster(): JSX.Element` - bottom-right, 4000 ms default, one per window.
  - `src/components/HoverDismissToaster.tsx`: `export default function HoverDismissToaster(): JSX.Element` (no props now).

- [ ] **Step 1: Write the failing test**

Create `src/lib/toast.test.tsx`:

```tsx
// The app's toasts: one local module every screen calls (lib/toast.ts),
// drawn by XiodUI's toast host (components/ui/toaster.tsx). The options the
// call sites were written against - sonner's - still mean what they meant.

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Toaster } from "../components/ui/toaster";
import { toast } from "./toast";

afterEach(() => {
  vi.useRealTimers();
});

test.each([
  ["success", toast.success],
  ["error", toast.error],
  ["info", toast.info],
  ["warning", toast.warning],
] as const)("toast.%s shows its message and description, marked with its kind", async (kind, show) => {
  render(<Toaster />);
  act(() => {
    show(`A ${kind} message`, { description: "More detail" });
  });
  const title = await screen.findByText(`A ${kind} message`);
  expect(screen.getByText("More detail")).toBeInTheDocument();
  expect(title.closest("[data-type]")).toHaveAttribute("data-type", kind);
  // Dragging a toast away must not highlight its text.
  expect(title.closest(".select-none")).not.toBeNull();
});

test("a plain toast() carries no kind", async () => {
  render(<Toaster />);
  act(() => {
    toast("Just so you know");
  });
  const title = await screen.findByText("Just so you know");
  expect(title.closest("[data-type]")).toBeNull();
});

test("an action runs once, and takes only its own toast away", async () => {
  const undo = vi.fn();
  render(<Toaster />);
  act(() => {
    toast.success("Saved elsewhere");
    toast.info("Changes discarded.", { action: { label: "Undo", onClick: undo } });
  });
  fireEvent.click(await screen.findByRole("button", { name: "Undo" }));
  expect(undo).toHaveBeenCalledTimes(1);
  await waitFor(() => expect(screen.queryByText("Changes discarded.")).not.toBeInTheDocument());
  expect(screen.getByText("Saved elsewhere")).toBeInTheDocument();
});

test("duration is how long a toast stays", async () => {
  render(<Toaster />);
  act(() => {
    toast.warning("Gone soon", { duration: 50 });
  });
  expect(await screen.findByText("Gone soon")).toBeInTheDocument();
  await waitFor(() => expect(screen.queryByText("Gone soon")).not.toBeInTheDocument(), { timeout: 2000 });
});

test("without a duration, a toast stays four seconds - sonner's default, which every message was written for", () => {
  vi.useFakeTimers();
  render(<Toaster />);
  act(() => {
    toast.success("Four seconds");
  });
  act(() => {
    vi.advanceTimersByTime(3_900);
  });
  expect(screen.getByText("Four seconds").closest("[data-ending-style]")).toBeNull();
  act(() => {
    vi.advanceTimersByTime(1_000);
  });
  const left = screen.queryByText("Four seconds");
  expect(left === null || left.closest("[data-ending-style]") !== null).toBe(true);
});

test("dismiss() with no id clears every toast", async () => {
  render(<Toaster />);
  act(() => {
    toast.info("One");
    toast.info("Two");
  });
  await screen.findByText("Two");
  act(() => {
    toast.dismiss();
  });
  await waitFor(() => {
    expect(screen.queryByText("One")).not.toBeInTheDocument();
    expect(screen.queryByText("Two")).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `npx vitest run src/lib/toast.test.tsx --exclude "**/.claude/**"`
Expected: FAIL - `Failed to resolve import "../components/ui/toaster"` / `"./toast"`.

- [ ] **Step 3: Write the toast module**

Create `src/lib/toast.ts`:

```ts
/**
 * The app's toasts. Every screen imports `toast` from HERE - never from a
 * library - so the library behind it is one file's business.
 *
 * It forwards to XiodUI's toast manager (Base UI underneath). The options
 * are the ones the call sites were written against when this was sonner:
 * a message, an optional description, a duration in ms, and an optional
 * action button. The host that draws them is components/ui/toaster.tsx.
 *
 * Exports `toast` only: test files mock this module as `{ toast: {...} }`,
 * and a named export they do not provide would throw when read.
 */
import type { ReactNode } from "react";
import { toastManager } from "xiod-ui/toast";

export type ToastOptions = {
  /** A second line under the message. */
  description?: ReactNode;
  /** How long it stays, in ms. Left out, the host's default (4 s). */
  duration?: number;
  /** One button on the toast. Clicking it runs `onClick` and dismisses the
   * toast, as sonner's action did. */
  action?: { label: ReactNode; onClick: () => void };
};

type Kind = "success" | "error" | "info" | "warning";
type Show = (message: ReactNode, opts?: ToastOptions) => string;

let issued = 0;

function show(kind: Kind | undefined, message: ReactNode, opts: ToastOptions = {}): string {
  // The id is ours, not the manager's, because the action has to close
  // exactly this toast and is built before the manager would hand one back.
  issued += 1;
  const id = `app-toast-${issued}`;
  const { action } = opts;
  toastManager.add({
    id,
    type: kind,
    title: message,
    description: opts.description,
    timeout: opts.duration,
    actionProps: action
      ? {
          children: action.label,
          onClick: () => {
            action.onClick();
            toastManager.close(id);
          },
        }
      : undefined,
  });
  return id;
}

export const toast: Show & {
  success: Show;
  error: Show;
  info: Show;
  warning: Show;
  dismiss: (id?: string) => void;
} = Object.assign((message: ReactNode, opts?: ToastOptions) => show(undefined, message, opts), {
  success: (message: ReactNode, opts?: ToastOptions) => show("success", message, opts),
  error: (message: ReactNode, opts?: ToastOptions) => show("error", message, opts),
  info: (message: ReactNode, opts?: ToastOptions) => show("info", message, opts),
  warning: (message: ReactNode, opts?: ToastOptions) => show("warning", message, opts),
  /** One toast by id, or every toast. */
  dismiss: (id?: string) => toastManager.close(id),
});
```

- [ ] **Step 4: Write the host**

Create `src/components/ui/toaster.tsx`:

```tsx
import { ToastProvider } from "xiod-ui/toast";

/** Sonner's default - the length every message in the app was written to
 * be read in. A call site that needs longer passes `duration`. */
const TOAST_MS = 4_000;

/**
 * The window's one toast host: XiodUI's, bottom-right, where the app's
 * toasts have always come up. Mount it once per window (App, RunnerWindow
 * via HoverDismissToaster); `toast` from lib/toast.ts reaches it from
 * anywhere. Colours come from the app's tokens through src/xiod-theme.css.
 */
export function Toaster() {
  return <ToastProvider position="bottom-right" timeout={TOAST_MS} />;
}
```

- [ ] **Step 5: Run the new test to verify it passes**

Run: `npx vitest run src/lib/toast.test.tsx --exclude "**/.claude/**"`
Expected: PASS (9 tests: the 4 kinds, then 5 more).

- [ ] **Step 6: Rewrite the runner's host and its test**

Replace the whole of `src/components/HoverDismissToaster.tsx` with:

```tsx
// A Toaster whose toasts leave when the pointer reaches them.
//
// XiodUI's toaster, like sonner's before it, PAUSES a toast on hover so it
// can be read - right for a message worth reading and wrong for the runner,
// where the toast is a two-word confirmation ("Pasted screenshot") that
// lands on top of the outcome buttons. There, a hand moving toward the
// button is the signal that the toast has been seen: it goes, and the
// button under it is live again without waiting or dragging.

import { toast } from "../lib/toast";
import { Toaster } from "./ui/toaster";

export default function HoverDismissToaster() {
  return (
    // React synthesises mouseenter along the React tree, portals included,
    // so entering a toast (portalled to <body>) fires this even though the
    // wrapper itself has no size.
    <div onMouseEnter={() => toast.dismiss()}>
      <Toaster />
    </div>
  );
}
```

Replace the whole of `src/components/HoverDismissToaster.test.tsx` with (markup changed: the host takes no props now, and toasts come from lib/toast):

```tsx
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test } from "vitest";
import { toast } from "../lib/toast";
import HoverDismissToaster from "./HoverDismissToaster";

/// The app's toaster pauses a toast on hover; this one does the opposite.
/// The runner's "Pasted screenshot" lands on the outcome buttons, so the
/// pointer arriving is the cue to get out of the way.
test("a toast leaves when the pointer reaches it", async () => {
  render(<HoverDismissToaster />);
  act(() => {
    toast.success("Pasted screenshot");
  });
  const el = await screen.findByText("Pasted screenshot");

  fireEvent.mouseEnter(el);
  await waitFor(() => expect(screen.queryByText("Pasted screenshot")).not.toBeInTheDocument());
});
```

- [ ] **Step 7: Move every source import to the local module**

`src/App.tsx`:
- replace `import { Toaster, toast } from "sonner";` with
  ```tsx
  import { toast } from "./lib/toast";
  import { Toaster } from "./components/ui/toaster";
  ```
- replace `import { getTheme, initTheme } from "./lib/theme";` with `import { initTheme } from "./lib/theme";` (the Toaster was `getTheme`'s only use in App);
- replace
  ```tsx
      {/* select-none: dragging a toast to dismiss must not highlight its text. */}
      <Toaster
        theme={getTheme() === "light" ? "light" : "dark"}
        richColors
        position="bottom-right"
        toastOptions={{ className: "select-none" }}
      />
  ```
  with
  ```tsx
      {/* The window's one toast host. XiodUI's toasts are select-none
          already, so dragging one away highlights nothing. */}
      <Toaster />
  ```

`src/screens/RunnerWindow.tsx`:
- replace `import { toast } from "sonner";` with `import { toast } from "../lib/toast";`
- delete the line `import { getTheme } from "../lib/theme";` (its only use was the toaster);
- replace `      <HoverDismissToaster theme={getTheme() === "light" ? "light" : "dark"} richColors position="bottom-right" />` with `      <HoverDismissToaster />`.

In each file below, replace the line `import { toast } from "sonner";` with `import { toast } from "<path>";`:

| File | `<path>` |
|---|---|
| `src/components/BugDialog.tsx` | `../lib/toast` |
| `src/components/BulkEditDialog.tsx` | `../lib/toast` |
| `src/components/CommentsPanel.tsx` | `../lib/toast` |
| `src/components/DeleteConfirm.tsx` | `../lib/toast` |
| `src/components/NotificationBell.tsx` | `../lib/toast` |
| `src/components/PipelineDialog.tsx` | `../lib/toast` |
| `src/components/PowerRenameDialog.tsx` | `../lib/toast` |
| `src/components/PrThreads.tsx` | `../lib/toast` |
| `src/components/QueueSection.tsx` | `../lib/toast` |
| `src/components/RelinkDialog.tsx` | `../lib/toast` |
| `src/components/WorkItemDrawer.tsx` | `../lib/toast` |
| `src/dev/DevPanel.tsx` | `../lib/toast` |
| `src/lib/updateToast.ts` | `./toast` |
| `src/screens/AiBridge.tsx` | `../lib/toast` |
| `src/screens/CreateWorkItem.tsx` | `../lib/toast` |
| `src/screens/ImportFile.tsx` | `../lib/toast` |
| `src/screens/PrPanel.tsx` | `../lib/toast` |
| `src/screens/Settings.tsx` | `../lib/toast` |
| `src/screens/Suites.tsx` | `../lib/toast` |
| `src/screens/WorkBoard.tsx` | `../lib/toast` |
| `src/screens/AutoRun/AccountsDialog.tsx` | `../../lib/toast` |
| `src/screens/AutoRun/index.tsx` | `../../lib/toast` |
| `src/screens/AutoRun/RecipeEditor.tsx` | `../../lib/toast` |
| `src/screens/AutoRun/RunPane.tsx` | `../../lib/toast` |
| `src/screens/AutoRun/RunReview.tsx` | `../../lib/toast` |
| `src/screens/AutoRun/ScriptEditor.tsx` | `../../lib/toast` |
| `src/screens/ExistingCases/CaseEditor.tsx` | `../../lib/toast` |
| `src/screens/ExistingCases/index.tsx` | `../../lib/toast` |
| `src/screens/ManageCases/NewSuiteDialog.tsx` | `../../lib/toast` |
| `src/screens/ManageCases/PlanTable.tsx` | `../../lib/toast` |
| `src/screens/ManageCases/SuiteCases.tsx` | `../../lib/toast` |
| `src/screens/RunPanel/CasePreview.tsx` | `../../lib/toast` |
| `src/screens/RunPanel/ExecutionOrderModal.tsx` | `../../lib/toast` |
| `src/screens/RunPanel/index.tsx` | `../../lib/toast` |
| `src/screens/RunPanel/useRunOrder.ts` | `../../lib/toast` |
| `src/screens/ViewCases/index.tsx` | `../../lib/toast` |

- [ ] **Step 8: Move every test import and mock to the local module**

(Markup/module change only: the same assertions against the same calls.)

In each file below, replace every occurrence of `"sonner"` (the import line and the `vi.mock("sonner", ...)` line alike; Edit with `replace_all: true`) with the quoted path shown:

| File | replace `"sonner"` with |
|---|---|
| `src/components/QueueSection.test.tsx` | `"../lib/toast"` |
| `src/lib/updateToast.test.ts` | `"./toast"` |
| `src/screens/PrPanel.test.tsx` | `"../lib/toast"` |
| `src/screens/RunPanel.test.tsx` | `"../lib/toast"` |
| `src/screens/AutoRun/AccountsDialog.test.tsx` | `"../../lib/toast"` |
| `src/screens/AutoRun/index.test.tsx` | `"../../lib/toast"` |
| `src/screens/AutoRun/RecipeEditor.test.tsx` | `"../../lib/toast"` |
| `src/screens/AutoRun/ScriptEditor.test.tsx` | `"../../lib/toast"` |
| `src/screens/ManageCases/index.test.tsx` | `"../../lib/toast"` |
| `src/screens/ManageCases/SuiteCases.test.tsx` | `"../../lib/toast"` |
| `src/screens/ManageCases/suites.test.tsx` | `"../../lib/toast"` |
| `src/screens/RunPanel/ExecutionOrderModal.test.tsx` | `"../../lib/toast"` |
| `src/screens/RunPanel/useRunOrder.test.tsx` | `"../../lib/toast"` |

In each file below, replace `import { Toaster } from "sonner";` with the line shown (the tests keep rendering `<Toaster />` unchanged):

| File | new line |
|---|---|
| `src/components/CommentsPanel.test.tsx` | `import { Toaster } from "./ui/toaster";` |
| `src/screens/AiBridge.test.tsx` | `import { Toaster } from "../components/ui/toaster";` |
| `src/screens/AutoRun.test.tsx` | `import { Toaster } from "../components/ui/toaster";` |
| `src/screens/ImportFile.test.tsx` | `import { Toaster } from "../components/ui/toaster";` |
| `src/screens/AutoRun/RunReview.test.tsx` | `import { Toaster } from "../../components/ui/toaster";` |

- [ ] **Step 9: Nothing imports sonner any more**

Grep (tool) pattern `from "sonner"|vi\.mock\("sonner"` over `src`: no matches. If any appear, move them as in Steps 7-8.

- [ ] **Step 10: Drop sonner's CSS override**

Edit `src/index.css`: delete this whole block (it styled sonner's action button, which no longer exists; XiodUI's action is drawn in the accent by the bridge):

```css
/* A toast's action button ("Undo" on a discard) is the app's own outline
   button, not sonner's.

   Sonner paints it `background: var(--normal-text)` over
   `color: var(--normal-bg)` - a solid white pill with black text, which
   under richColors is the one element on a toast that ignores both the
   theme and the accent the user picked. Next to every other button in the
   app it reads as a piece of somebody else's UI.

   This mirrors `Button variant="outline" size="sm"` (see ui/button.tsx)
   in plain CSS rather than via `toastOptions.classNames`, because those
   land as single utility classes and lose to sonner's own
   `[data-sonner-toast][data-styled='true'] [data-button]` on specificity.
   Matching that selector exactly is what makes the override stick.

   `[data-button]` covers the cancel button too, deliberately: two buttons
   on one toast styled differently would be worse than either choice. */
[data-sonner-toast][data-styled="true"] [data-button] {
  height: auto;
  padding: 0.375rem 0.75rem;
  border-radius: 0.375rem;
  border: 1px solid var(--color-border);
  background: transparent;
  color: var(--color-text);
  font-weight: 500;
  transition: color 150ms, border-color 150ms;
}

[data-sonner-toast][data-styled="true"] [data-button]:hover {
  border-color: var(--color-accent);
  color: var(--color-accent);
  background: transparent;
}

/* Sonner's default focus ring is a black box-shadow, invisible on a dark
   toast. The accent outline is what every other control in the app uses. */
[data-sonner-toast][data-styled="true"] [data-button]:focus-visible {
  box-shadow: none;
  outline: 2px solid var(--color-accent);
  outline-offset: 2px;
}
```

(Delete the blank line after it too, so one blank line separates the preceding `.nav-ico-on` rule from the `/* ── Motion` comment.)

- [ ] **Step 11: Remove sonner**

Run: `npm uninstall sonner`
Expected: `sonner` gone from `package.json` dependencies.

- [ ] **Step 12: Run everything that touches toasts**

Run: `npx vitest run src/lib/toast.test.tsx src/components/HoverDismissToaster.test.tsx src/lib/updateToast.test.ts src/components/QueueSection.test.tsx src/components/CommentsPanel.test.tsx src/screens/PrPanel.test.tsx src/screens/RunPanel.test.tsx src/screens/AiBridge.test.tsx src/screens/AutoRun.test.tsx src/screens/ImportFile.test.tsx src/screens/AutoRun src/screens/ManageCases src/screens/RunPanel src/App.test.tsx src/xiod.test.ts src/ui-consistency.test.ts --exclude "**/.claude/**"`
Expected: PASS. A test that found a toast by text still finds it (the title renders as text in an `<h2>`). If an unnamed `getByRole("dialog")` now finds a toast too (Base UI toasts are `role="dialog"`), give that query the dialog's name - do not remove the assertion.

- [ ] **Step 13: Typecheck**

Run: `npx tsc --noEmit`
Expected: no errors (in particular no unused `getTheme`).

- [ ] **Step 14: Commit**

```bash
git add package.json package-lock.json src/lib/toast.ts src/lib/toast.test.tsx src/components/ui/toaster.tsx src/components/HoverDismissToaster.tsx src/components/HoverDismissToaster.test.tsx src/App.tsx src/screens/RunnerWindow.tsx src/index.css src/components/BugDialog.tsx src/components/BulkEditDialog.tsx src/components/CommentsPanel.tsx src/components/DeleteConfirm.tsx src/components/NotificationBell.tsx src/components/PipelineDialog.tsx src/components/PowerRenameDialog.tsx src/components/PrThreads.tsx src/components/QueueSection.tsx src/components/RelinkDialog.tsx src/components/WorkItemDrawer.tsx src/dev/DevPanel.tsx src/lib/updateToast.ts src/screens/AiBridge.tsx src/screens/CreateWorkItem.tsx src/screens/ImportFile.tsx src/screens/PrPanel.tsx src/screens/Settings.tsx src/screens/Suites.tsx src/screens/WorkBoard.tsx src/screens/AutoRun/AccountsDialog.tsx src/screens/AutoRun/index.tsx src/screens/AutoRun/RecipeEditor.tsx src/screens/AutoRun/RunPane.tsx src/screens/AutoRun/RunReview.tsx src/screens/AutoRun/ScriptEditor.tsx src/screens/ExistingCases/CaseEditor.tsx src/screens/ExistingCases/index.tsx src/screens/ManageCases/NewSuiteDialog.tsx src/screens/ManageCases/PlanTable.tsx src/screens/ManageCases/SuiteCases.tsx src/screens/RunPanel/CasePreview.tsx src/screens/RunPanel/ExecutionOrderModal.tsx src/screens/RunPanel/index.tsx src/screens/RunPanel/useRunOrder.ts src/screens/ViewCases/index.tsx
git add src/components/QueueSection.test.tsx src/lib/updateToast.test.ts src/screens/PrPanel.test.tsx src/screens/RunPanel.test.tsx src/screens/AutoRun/AccountsDialog.test.tsx src/screens/AutoRun/index.test.tsx src/screens/AutoRun/RecipeEditor.test.tsx src/screens/AutoRun/ScriptEditor.test.tsx src/screens/ManageCases/index.test.tsx src/screens/ManageCases/SuiteCases.test.tsx src/screens/ManageCases/suites.test.tsx src/screens/RunPanel/ExecutionOrderModal.test.tsx src/screens/RunPanel/useRunOrder.test.tsx src/components/CommentsPanel.test.tsx src/screens/AiBridge.test.tsx src/screens/AutoRun.test.tsx src/screens/ImportFile.test.tsx src/screens/AutoRun/RunReview.test.tsx
git commit -F - <<'EOF'
feat(v2): toasts are XiodUI's, called the same way from every screen

Every screen now calls toast from src/lib/toast.ts, which forwards to
XiodUI's toast manager; one host per window (components/ui/toaster.tsx)
draws them bottom-right in the app's colours. Messages, descriptions,
durations and the Undo action keep their meaning, and the runner's toasts
still leave when the pointer reaches them. sonner is gone.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 3: Checkbox

**Files:**
- Modify (rewrite): `src/components/ui/checkbox.tsx`
- Modify: `src/components/ui/checkbox.test.tsx` (append tests)
- Modify: `src/components/ui/motion.test.tsx:33-43` (the checkbox test; markup changed)
- Modify: `src/components/PowerRenameDialog.test.tsx:69` (query changed; markup changed)
- Modify: `src/index.css` (the `.t-check` rules and their reduced-motion entries go)

**Interfaces:**
- Consumes: `Checkbox` from `xiod-ui/checkbox` (props: Base UI `Checkbox.Root.Props` + `variant`; `onCheckedChange(checked: boolean, details)`); bridge colours from Task 1.
- Produces: unchanged `export function Checkbox({ checked, indeterminate?, onCheckedChange, ariaLabel?, className? })`. Markup: `<span role="checkbox" data-slot="checkbox">` with a hidden `<input type="checkbox" aria-hidden tabindex=-1>` beside it; indicator `[data-slot="checkbox-indicator"]`.

- [ ] **Step 1: Write the failing tests**

Append to `src/components/ui/checkbox.test.tsx`:

```tsx
// A wrapping <label> is how most boxes in the app get their name
// (PowerRenameDialog, multiselect, BulkEditDialog): it must name the box,
// and one click - on the words or on the box - must toggle it once.
test("a wrapping label names the box, and clicking its text toggles once", () => {
  const onChange = vi.fn();
  render(
    <label>
      <Checkbox checked={false} onCheckedChange={onChange} />
      Apply module
    </label>,
  );
  expect(screen.getByRole("checkbox", { name: "Apply module" })).toHaveAttribute("data-slot", "checkbox");
  fireEvent.click(screen.getByText("Apply module"));
  expect(onChange).toHaveBeenCalledTimes(1);
  expect(onChange).toHaveBeenCalledWith(true);
});

test("clicking the box itself inside a label also toggles once", () => {
  const onChange = vi.fn();
  render(
    <label>
      <Checkbox checked={false} onCheckedChange={onChange} />
      Match case
    </label>,
  );
  fireEvent.click(screen.getByRole("checkbox", { name: "Match case" }));
  expect(onChange).toHaveBeenCalledTimes(1);
  expect(onChange).toHaveBeenCalledWith(true);
});

test("Space toggles it", () => {
  const onChange = vi.fn();
  render(<Checkbox checked={false} onCheckedChange={onChange} ariaLabel="Pick" />);
  const box = screen.getByRole("checkbox", { name: "Pick" });
  fireEvent.keyDown(box, { key: " " });
  fireEvent.keyUp(box, { key: " " });
  expect(onChange).toHaveBeenCalledWith(true);
});

// QueueRow puts a box inside a clickable row and relies on the row hearing
// the click - once.
test("a click on the box still reaches the row it sits in, once", () => {
  const row = vi.fn();
  const onChange = vi.fn();
  render(
    <div onClick={row}>
      <Checkbox checked onCheckedChange={onChange} ariaLabel="Select Login works" />
    </div>,
  );
  fireEvent.click(screen.getByRole("checkbox", { name: "Select Login works" }));
  expect(row).toHaveBeenCalledTimes(1);
  expect(onChange).toHaveBeenCalledWith(false);
});
```

- [ ] **Step 2: Run them to verify they fail**

Run: `npx vitest run src/components/ui/checkbox.test.tsx --exclude "**/.claude/**"`
Expected: FAIL - the first test fails on `toHaveAttribute("data-slot", "checkbox")` (today's box is a plain `<button>`); the other new tests may already pass against the old button, which is fine - they pin behaviour the swap must keep.

- [ ] **Step 3: Rewrite the wrapper**

Replace the whole of `src/components/ui/checkbox.tsx` with:

```tsx
import { Checkbox as XiodCheckbox } from "xiod-ui/checkbox";
import { cn } from "../../lib/cn";

/**
 * The app's checkbox: XiodUI's (Base UI underneath), drawn in the app's
 * tokens through src/xiod-theme.css. A `<span role="checkbox">` with a
 * hidden native input beside it, so a wrapping `<label>` both names it and
 * toggles it, Space toggles it, and a click still reaches the row it sits
 * in. The tick draws itself in and rubs itself out (XiodUI keeps it
 * mounted), and stands still under reduced motion (the bridge).
 */
export function Checkbox({
  checked,
  indeterminate = false,
  onCheckedChange,
  ariaLabel,
  className,
}: {
  checked: boolean;
  /** "Some but not all" - a dash and aria-checked="mixed". Only
   * meaningful while unchecked; clicking from mixed checks the rest
   * (onCheckedChange still receives true). */
  indeterminate?: boolean;
  onCheckedChange: (checked: boolean) => void;
  ariaLabel?: string;
  className?: string;
}) {
  return (
    <XiodCheckbox
      checked={checked}
      // Checked wins over a stale indeterminate flag.
      indeterminate={!checked && indeterminate}
      onCheckedChange={(next) => onCheckedChange(next)}
      aria-label={ariaLabel}
      // The empty box keeps the stronger border it always had: at 16px the
      // field border XiodUI uses (`border-input`) is too faint to find.
      className={cn("border-border-strong", className)}
    />
  );
}
```

- [ ] **Step 4: Update the motion test (markup changed)**

In `src/components/ui/motion.test.tsx`, replace the test `"the checkbox tick is always there to draw in; the mixed state shows a dash instead"` (lines 33-43) with the same claim against XiodUI's markup - the tick path is present while unchecked so checking can draw it, and the mixed state swaps it for a dash:

```tsx
test("the checkbox tick is always there to draw in; the mixed state shows a dash instead", () => {
  const TICK = "M5.252 12.7 10.2 18.63 18.748 5.37";
  const DASH = "M5.252 12h13.496";
  const mark = () =>
    screen
      .getByRole("checkbox", { name: "Pick" })
      .querySelector('[data-slot="checkbox-indicator"] path')
      ?.getAttribute("d");
  const { rerender } = render(<Checkbox checked={false} onCheckedChange={() => {}} ariaLabel="Pick" />);
  // Present while unchecked, so checking can DRAW it rather than pop it in.
  expect(mark()).toBe(TICK);
  rerender(<Checkbox checked onCheckedChange={() => {}} ariaLabel="Pick" />);
  expect(mark()).toBe(TICK);
  rerender(<Checkbox checked={false} indeterminate onCheckedChange={() => {}} ariaLabel="Pick" />);
  expect(mark()).toBe(DASH);
});
```

- [ ] **Step 5: Update the one label query (markup changed)**

In `src/components/PowerRenameDialog.test.tsx:69`, replace

```tsx
  fireEvent.click(screen.getByLabelText(/Regular expression/i));
```

with

```tsx
  fireEvent.click(screen.getByRole("checkbox", { name: /Regular expression/i }));
```

(The `<label>` now labels two elements - the visible box via `aria-labelledby` and its hidden native input - so a label query finds both; the role query finds the one a person clicks.)

- [ ] **Step 6: Remove the old checkbox motion from index.css**

Edit `src/index.css`:

1. Replace
```css
/* ── Motion ─────────────────────────────────────────────────────────────
   Transitions adapted from transitions.dev's free set (MIT, the
   `transitions-dev` package): menu dropdown, modal, toggle and checkbox
   check. One scale of durations and easings so the controls that share a
```
with
```css
/* ── Motion ─────────────────────────────────────────────────────────────
   Transitions adapted from transitions.dev's free set (MIT, the
   `transitions-dev` package): menu dropdown, modal and toggle. (The
   checkbox's own motion is XiodUI's - see src/xiod-theme.css.) One scale
   of durations and easings so the controls that share a
```

2. Replace
```css
  --motion-quick: 150ms; /* dropdown / modal close, checkbox fill */
```
with
```css
  --motion-quick: 150ms; /* dropdown / modal close */
```
and
```css
  --motion-draw: 350ms; /* checkbox tick draw, switch travel */
```
with
```css
  --motion-draw: 350ms; /* switch travel */
```

3. Delete this block entirely:
```css
/* Checkbox check: the box fills, then the tick draws itself. Unchecking
   dissolves it: the tick fades as it retracts and its colour follows the
   box's, so it never sits white on a box that has already gone pale. The dash is the tick path's own length rounded up
   (14.4 -> 15), so it never over- or under-draws; the rest offset sits a
   unit past it, so the round line cap cannot leave a dot on an empty box. */
.t-check {
  transition:
    background-color var(--motion-quick) var(--motion-ease-smooth-out),
    border-color var(--motion-quick) var(--motion-ease-smooth-out),
    color var(--motion-quick) var(--motion-ease-smooth-out);
}
.t-check .t-check-tick {
  stroke-dasharray: 15 20;
  stroke-dashoffset: 16;
  opacity: 0;
  transition:
    stroke-dashoffset var(--motion-quick) var(--motion-ease-smooth-out),
    opacity var(--motion-quick) var(--motion-ease-smooth-out);
}
.t-check[aria-checked="true"] .t-check-tick {
  stroke-dashoffset: 0;
  opacity: 1;
  transition: stroke-dashoffset var(--motion-draw) var(--motion-ease-smooth-out);
}

```

4. Replace
```css
  .t-dropdown.is-closing,
  .t-check,
  .t-check .t-check-tick {
    transition: none !important;
  }
```
with
```css
  .t-dropdown.is-closing {
    transition: none !important;
  }
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `npx vitest run src/components/ui/checkbox.test.tsx src/components/ui/motion.test.tsx src/components/PowerRenameDialog.test.tsx src/a11y.test.tsx src/xiod.test.ts src/ui-consistency.test.ts --exclude "**/.claude/**"`
Expected: PASS.

- [ ] **Step 8: Full suite (checkboxes are on most screens)**

Run: `npx vitest run --exclude "**/.claude/**"`
Expected: PASS. If a test fails with "Found multiple elements with the text of: <label text>" from `getByLabelText` on a label that wraps a Checkbox, change that query to `getByRole("checkbox", { name: ... })` as in Step 5 (same markup reason) and re-run. Any other failure: investigate, do not loosen.

- [ ] **Step 9: Typecheck**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 10: Commit**

```bash
git add src/components/ui/checkbox.tsx src/components/ui/checkbox.test.tsx src/components/ui/motion.test.tsx src/components/PowerRenameDialog.test.tsx src/index.css
git commit -F - <<'EOF'
feat(v2): checkboxes are XiodUI's, still named by their labels

The box, its tick and its mixed-state dash are XiodUI's, in the app's
colours. A wrapping label still names and toggles it, Space toggles it,
a click still reaches the row it sits in, and it stands still when the
system asks for less motion.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

(If Step 8 changed further test files, `git add` each by path before committing.)

---

### Task 4: Textarea

**Files:**
- Modify: `src/components/ui/input.tsx` (the `Textarea` export; `Input` unchanged)
- Create: `src/components/ui/input.test.tsx`

**Interfaces:**
- Consumes: `Textarea` from `xiod-ui/textarea` (`React.ComponentProps<"textarea"> & { size?, unstyled? }`; renders `<span data-slot="textarea-control">` (receives `className`) wrapping `<textarea data-slot="textarea">` (receives every other prop, `ref` included)).
- Produces: unchanged `export const Textarea = forwardRef<HTMLTextAreaElement, TextareaHTMLAttributes<HTMLTextAreaElement>>`. A call site's `className` now sizes the styled box; the field fills it.

- [ ] **Step 1: Write the failing tests**

Create `src/components/ui/input.test.tsx`:

```tsx
// Textarea is XiodUI's: a styled box (data-slot="textarea-control") with the
// <textarea> filling it. Call sites size the BOX - h-40, w-full, flex-1,
// min-h-... - exactly as they sized the old bare textarea, and never change.

import { fireEvent, render, screen } from "@testing-library/react";
import { createRef } from "react";
import { expect, test, vi } from "vitest";
import { Textarea } from "./input";

const box = (name: string) => screen.getByRole("textbox", { name }).parentElement!;

test("the call site's classes size the box; the field inside fills it", () => {
  render(<Textarea aria-label="Notes" className="h-40 w-full font-mono" />);
  const field = screen.getByRole("textbox", { name: "Notes" });
  expect(field.tagName).toBe("TEXTAREA");
  expect(field).toHaveAttribute("data-slot", "textarea");
  expect(box("Notes")).toHaveAttribute("data-slot", "textarea-control");
  expect(box("Notes")).toHaveClass("h-40", "w-full", "font-mono");
  expect(box("Notes")).toHaveClass("[&>textarea]:h-full", "[&>textarea]:min-h-0", "[&>textarea]:[field-sizing:fixed]");
});

test("the box is what the user drags taller", () => {
  render(<Textarea aria-label="Notes" />);
  expect(box("Notes")).toHaveClass("resize-y", "overflow-hidden", "[&>textarea]:resize-none");
});

// XiodUI sets `sm:text-sm` on the box, and a breakpoint class outranks a
// plain one - so without its twin, a call site's text-xs would lose in any
// window wider than 640px (BugDialog's repro steps, RunPane, RecipeEditor).
test("a call site's text size survives XiodUI's own size at sm: and up", () => {
  render(<Textarea aria-label="Repro steps" className="h-40 w-full font-mono text-xs" />);
  expect(box("Repro steps")).toHaveClass("text-xs", "sm:text-xs");
  expect(box("Repro steps")).not.toHaveClass("sm:text-sm");
  expect(box("Repro steps")).not.toHaveClass("text-base");
});

test("with no size given it reads at text-sm, like an Input beside it", () => {
  render(<Textarea aria-label="Notes" />);
  expect(box("Notes")).toHaveClass("text-sm", "sm:text-sm");
  expect(box("Notes")).not.toHaveClass("text-base");
});

// MarkdownField edits the selection through this ref.
test("the forwarded ref is the <textarea> itself", () => {
  const ref = createRef<HTMLTextAreaElement>();
  render(<Textarea ref={ref} aria-label="Notes" defaultValue="hello" />);
  expect(ref.current).toBe(screen.getByRole("textbox", { name: "Notes" }));
  expect(ref.current?.value).toBe("hello");
});

test("value and onChange reach the field", () => {
  const onChange = vi.fn();
  render(<Textarea aria-label="Notes" value="a" onChange={onChange} />);
  const field = screen.getByRole("textbox", { name: "Notes" });
  expect(field).toHaveValue("a");
  fireEvent.change(field, { target: { value: "ab" } });
  expect(onChange).toHaveBeenCalledTimes(1);
});

test("disabled reaches the field", () => {
  render(<Textarea aria-label="Notes" disabled />);
  expect(screen.getByRole("textbox", { name: "Notes" })).toBeDisabled();
});
```

- [ ] **Step 2: Run them to verify they fail**

Run: `npx vitest run src/components/ui/input.test.tsx --exclude "**/.claude/**"`
Expected: FAIL - `toHaveAttribute("data-slot", "textarea")` and the box assertions fail (today the textarea has no wrapper and no data-slot).

- [ ] **Step 3: Implement**

In `src/components/ui/input.tsx`, replace

```tsx
import { forwardRef, type InputHTMLAttributes, type TextareaHTMLAttributes } from "react";
import { cn } from "../../lib/cn";
```

with

```tsx
import { forwardRef, type InputHTMLAttributes, type TextareaHTMLAttributes } from "react";
import { Textarea as XiodTextarea } from "xiod-ui/textarea";
import { cn } from "../../lib/cn";
```

and replace

```tsx
export const Textarea = forwardRef<
  HTMLTextAreaElement,
  TextareaHTMLAttributes<HTMLTextAreaElement>
>(({ className, ...props }, ref) => (
  <textarea ref={ref} className={cn(base, className)} {...props} />
));
Textarea.displayName = "Textarea";
```

with

```tsx
/** XiodUI's text area sets its own size at sm: and up (`sm:text-sm`), and a
 * breakpoint class always outranks a plain one - so a call site's `text-xs`
 * would lose in any window wider than 640px. Its sm: twin goes along with
 * it. Whole class names, so Tailwind finds them in this file. */
const SM_TWIN: Record<string, string> = {
  "text-xs": "sm:text-xs",
  "text-sm": "sm:text-sm",
  "text-base": "sm:text-base",
  "text-lg": "sm:text-lg",
};

function smTwin(className?: string): string | undefined {
  const size = className?.split(/\s+/).find((c) => c in SM_TWIN);
  return size ? SM_TWIN[size] : undefined;
}

/**
 * XiodUI's text area: a styled box with the field filling it. The call
 * site's `className` sizes the BOX (h-*, w-*, flex-1, min-h-*, margins, a
 * ring), which is also what the user drags taller; the field fills it and
 * does not grow with its content. Every other prop, `ref` included, lands
 * on the `<textarea>`.
 */
export const Textarea = forwardRef<
  HTMLTextAreaElement,
  TextareaHTMLAttributes<HTMLTextAreaElement>
>(({ className, ...props }, ref) => (
  <XiodTextarea
    ref={ref}
    className={cn(
      "flex resize-y overflow-hidden text-sm [&>textarea]:h-full [&>textarea]:min-h-0 [&>textarea]:resize-none [&>textarea]:[field-sizing:fixed] [&>textarea]:placeholder:text-faint",
      className,
      smTwin(className),
    )}
    {...props}
  />
));
Textarea.displayName = "Textarea";
```

(`base` stays: `Input` still uses it.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npx vitest run src/components/ui/input.test.tsx src/components/MarkdownField.test.tsx src/a11y.test.tsx src/xiod.test.ts src/ui-consistency.test.ts --exclude "**/.claude/**"`
Expected: PASS. (If `src/components/MarkdownField.test.tsx` does not exist, drop it from the command.)

- [ ] **Step 5: Full suite (text areas are on many screens)**

Run: `npx vitest run --exclude "**/.claude/**"`
Expected: PASS.

- [ ] **Step 6: Typecheck**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 7: Commit**

```bash
git add src/components/ui/input.tsx src/components/ui/input.test.tsx
git commit -F - <<'EOF'
feat(v2): text areas are XiodUI's, sized where they always were

Every text area is XiodUI's, in the app's colours, with the same height,
width and text size each screen gave it, and still drags taller.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 5: Command palette list and key hints

**Files:**
- Create: `src/components/ui/kbd.tsx`, `src/components/ui/kbd.test.tsx`
- Modify (rewrite): `src/components/CommandPalette.tsx`
- Modify: `src/components/CommandPalette.test.tsx` (line 72 selector; markup changed; tests appended)
- Modify: `src/test-setup.ts:20` (comment)
- Modify: `package.json`, `package-lock.json` (cmdk removed)

**Interfaces:**
- Consumes: `Kbd`, `KbdGroup` from `xiod-ui/kbd`; `Command`, `CommandCollection`, `CommandDialog`, `CommandDialogPopup`, `CommandEmpty`, `CommandGroup`, `CommandGroupLabel`, `CommandInput`, `CommandItem`, `CommandList`, `CommandPanel` from `xiod-ui/command` (Base UI Autocomplete + Dialog underneath; items given as `items={groups}` where a group is `{ value, items }`; `CommandList` / `CommandCollection` take render functions; `CommandItem` gets `value={item}` and `onClick`; rows carry `data-slot="command-item"` and `data-highlighted` when highlighted).
- Produces: `export function Kbd({ keys, className }: { keys: string; className?: string })` in `src/components/ui/kbd.tsx` - same `keys` strings as Astryx's (`"mod+1"`, `"mod+shift+m"`), a `role="img"` group named in words ("Control + Shift + M") with one `data-slot="kbd"` cap per key. `CommandPalette`'s props and shortcuts are unchanged.

- [ ] **Step 1: Write the failing Kbd test**

Create `src/components/ui/kbd.test.tsx`:

```tsx
// Keyboard hints: one XiodUI key cap per key, read out as words - a screen
// reader reads glyphs like the shift arrow as nonsense.

import { render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Kbd } from "./kbd";

afterEach(() => {
  vi.restoreAllMocks();
});

test("one key cap per key, read out as words", () => {
  render(<Kbd keys="mod+shift+m" />);
  const hint = screen.getByRole("img", { name: "Control + Shift + M" });
  const caps = [...hint.querySelectorAll('[data-slot="kbd"]')];
  expect(caps.map((c) => c.textContent)).toEqual(["Ctrl", "⇧", "M"]);
  for (const c of caps) expect(c).toHaveAttribute("aria-hidden", "true");
});

test("mod is Command on a Mac", () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
  render(<Kbd keys="mod+k" />);
  expect(screen.getByRole("img", { name: "Command + K" })).toHaveTextContent("⌘K");
});
```

- [ ] **Step 2: Write the failing palette tests**

In `src/components/CommandPalette.test.tsx`:

1. Change the first import line to add `waitFor`:
```tsx
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
```
2. Replace (markup changed: rows are XiodUI's, not cmdk's)
```tsx
    const row = screen.getByText(item.label).closest("[cmdk-item]");
```
with
```tsx
    const row = screen.getByText(item.label).closest('[data-slot="command-item"]');
```
3. Append:
```tsx
test("typing narrows the list; a query nothing matches says so, and Enter then runs nothing", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_projects") return [];
  });
  const { onNavigate, onToggleWork } = renderPalette();
  fireEvent.keyDown(window, { key: "k", ctrlKey: true });
  const input = await screen.findByPlaceholderText(/Type a command/);

  fireEvent.change(input, { target: { value: "sett" } });
  expect(await screen.findByText("Settings")).toBeInTheDocument();
  await waitFor(() => expect(screen.queryByText("Toggle Work Manager")).not.toBeInTheDocument());

  fireEvent.change(input, { target: { value: "zzzz" } });
  expect(await screen.findByText("No results.")).toBeInTheDocument();
  fireEvent.keyDown(input, { key: "Enter" });
  expect(onNavigate).not.toHaveBeenCalled();
  expect(onToggleWork).not.toHaveBeenCalled();
});

test("Enter runs the highlighted row - the first, as the palette opens - and closes it", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_projects") return [];
  });
  const { onNavigate } = renderPalette();
  fireEvent.keyDown(window, { key: "k", ctrlKey: true });
  const input = await screen.findByPlaceholderText(/Type a command/);
  const first = VISIBLE_CASE_ITEMS[0];
  await waitFor(() =>
    expect(screen.getByText(first.label).closest('[data-slot="command-item"]')).toHaveAttribute("data-highlighted"),
  );

  fireEvent.keyDown(input, { key: "Enter" });
  expect(onNavigate).toHaveBeenCalledWith(first.id);
  await waitFor(() => expect(screen.queryByPlaceholderText(/Type a command/)).not.toBeInTheDocument());
});
```

- [ ] **Step 3: Run them to verify they fail**

Run: `npx vitest run src/components/ui/kbd.test.tsx src/components/CommandPalette.test.tsx --exclude "**/.claude/**"`
Expected: FAIL - `Failed to resolve import "./kbd"`; the palette's hint test fails (`closest('[data-slot="command-item"]')` is null on cmdk rows); the Enter test fails (no `data-highlighted`).

- [ ] **Step 4: Write the Kbd wrapper**

Create `src/components/ui/kbd.tsx`:

```tsx
import { Kbd as XiodKey, KbdGroup } from "xiod-ui/kbd";

/** How each key is DRAWN. `mod` is resolved per platform below. */
const GLYPH: Record<string, string> = {
  ctrl: "⌃",
  alt: "⌥",
  shift: "⇧",
  enter: "↵",
  backspace: "⌫",
  escape: "Esc",
  tab: "⇥",
  up: "↑",
  down: "↓",
  left: "←",
  right: "→",
  plus: "+",
};

/** How each key is SPOKEN - glyphs mean nothing to a screen reader. */
const SPOKEN: Record<string, string> = {
  ctrl: "Control",
  alt: "Alt",
  shift: "Shift",
  enter: "Enter",
  backspace: "Backspace",
  escape: "Escape",
  tab: "Tab",
  up: "Up arrow",
  down: "Down arrow",
  left: "Left arrow",
  right: "Right arrow",
  plus: "Plus",
};

function isMac(): boolean {
  if (typeof navigator === "undefined") return false;
  const hints = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData;
  if (hints?.platform !== undefined) return /mac/i.test(hints.platform);
  return /Mac|iPhone|iPad|iPod/.test(navigator.platform ?? "");
}

const glyph = (key: string, mac: boolean) =>
  key === "mod" ? (mac ? "⌘" : "Ctrl") : (GLYPH[key] ?? key.toUpperCase());
const spoken = (key: string, mac: boolean) =>
  key === "mod" ? (mac ? "Command" : "Control") : (SPOKEN[key] ?? key.toUpperCase());

/**
 * A keyboard shortcut, e.g. `<Kbd keys="mod+shift+m" />`: one XiodUI key cap
 * per key, announced in words ("Control + Shift + M"). It takes the same
 * `keys` strings the Astryx Kbd did, so Sidebar's `sectionShortcut` feeds it
 * unchanged. `mod` is Ctrl, or the Command key on a Mac.
 */
export function Kbd({ keys, className }: { keys: string; className?: string }) {
  const mac = isMac();
  const parts = keys.split("+").map((k) => k.trim().toLowerCase());
  return (
    <KbdGroup role="img" aria-label={parts.map((k) => spoken(k, mac)).join(" + ")} className={className}>
      {parts.map((k) => (
        <XiodKey key={k} aria-hidden="true">
          {glyph(k, mac)}
        </XiodKey>
      ))}
    </KbdGroup>
  );
}
```

- [ ] **Step 5: Rewrite the palette on XiodUI's command list**

Replace the whole of `src/components/CommandPalette.tsx` with:

```tsx
import { reportUpdateCheck } from "../lib/updateToast";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Command,
  CommandCollection,
  CommandDialog,
  CommandDialogPopup,
  CommandEmpty,
  CommandGroup,
  CommandGroupLabel,
  CommandInput,
  CommandItem,
  CommandList,
  CommandPanel,
} from "xiod-ui/command";
import { useEffect, useState } from "react";
import { commands } from "../bindings";
import { Kbd } from "./ui/kbd";
import { unwrap } from "../lib/ipc";
import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";
import { getTheme, setTheme } from "../lib/theme";
import { tourRunningSnapshot } from "../tour/tourState";
import { VISIBLE_CASE_ITEMS, sectionShortcut, type Section } from "./Sidebar";

/** One row. `value` is unique across the whole palette; `label` is what
 * shows and what typing filters on; `keys` is its shortcut hint. */
type Entry = { value: string; label: string; keys?: string; run: () => void };
type Group = { value: string; items: Entry[] };

export default function CommandPalette({
  onNavigate,
  org,
  onSwitchProject,
  onToggleWork,
}: {
  onNavigate: (s: Section) => void;
  org: string;
  onSwitchProject: (p: string) => void;
  onToggleWork: () => void;
}) {
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (tourRunningSnapshot()) return;
      if (e.key.toLowerCase() === "k" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Same key + cache as ContextBar, so the palette never refetches what
  // the bar already has.
  const projects = useQuery({
    queryKey: ["projects", org],
    ...persistentQuery({
      key: cacheKeys.projects(org),
      fetcher: () => unwrap(commands.listProjects(org)),
      ...CACHE.reference,
    }),
    enabled: open && Boolean(org),
  });

  const run = (fn: () => void) => {
    fn();
    setOpen(false);
  };

  const groups: Group[] = [
    {
      value: "Go to",
      items: [
        // One row per sidebar tab, hint digit = its Ctrl+N slot, both read
        // off the same list App's shortcut handler uses.
        ...VISIBLE_CASE_ITEMS.map((i) => ({
          value: `go:${i.id}`,
          label: i.label,
          keys: sectionShortcut(i.id),
          run: () => onNavigate(i.id),
        })),
        { value: "go:settings", label: "Settings", run: () => onNavigate("settings") },
      ],
    },
    {
      value: "Actions",
      items: [
        { value: "action:work", label: "Toggle Work Manager", keys: "mod+shift+m", run: onToggleWork },
        {
          value: "action:theme",
          label: "Toggle theme",
          run: () => setTheme(getTheme() === "light" ? "dark" : "light"),
        },
        {
          value: "action:update",
          label: "Check for updates",
          run: async () => {
            const v = await commands.checkUpdate();
            // Seed the ["update"] query so App's update banner appears.
            qc.setQueryData(["update"], v);
            reportUpdateCheck(v);
          },
        },
      ],
    },
  ];
  if (org && (projects.data?.length ?? 0) > 0) {
    groups.push({
      value: "Switch project",
      items: projects.data!.map((p) => ({ value: `project:${p.id}`, label: p.name, run: () => onSwitchProject(p.name) })),
    });
  }

  return (
    <CommandDialog open={open} onOpenChange={setOpen}>
      <CommandDialogPopup aria-label="Command palette">
        <Command items={groups}>
          <CommandInput placeholder="Type a command or search" />
          <CommandPanel>
            <CommandEmpty>No results.</CommandEmpty>
            <CommandList>
              {(group: Group) => (
                <CommandGroup key={group.value} items={group.items}>
                  <CommandGroupLabel className="text-[10px] uppercase tracking-wide text-faint">
                    {group.value}
                  </CommandGroupLabel>
                  <CommandCollection>
                    {(item: Entry) => (
                      <CommandItem
                        key={item.value}
                        value={item}
                        onClick={() => run(item.run)}
                        className="cursor-pointer justify-between gap-3 text-text"
                      >
                        {item.label}
                        {item.keys && <Kbd keys={item.keys} />}
                      </CommandItem>
                    )}
                  </CommandCollection>
                </CommandGroup>
              )}
            </CommandList>
          </CommandPanel>
        </Command>
      </CommandDialogPopup>
    </CommandDialog>
  );
}
```

- [ ] **Step 6: Update the jsdom-gap comment**

In `src/test-setup.ts`, replace `// jsdom gaps that cmdk relies on.` with `// jsdom gaps that Base UI (the command palette's list and its scroll area) relies on.`

- [ ] **Step 7: Run the tests to verify they pass**

Run: `npx vitest run src/components/ui/kbd.test.tsx src/components/CommandPalette.test.tsx src/App.test.tsx src/xiod.test.ts src/ui-consistency.test.ts --exclude "**/.claude/**"`
Expected: PASS (App.test's "Ctrl+K must not open" lock test included).

- [ ] **Step 8: Remove cmdk**

Grep (tool) pattern `cmdk` over `src` (glob `*.{ts,tsx}`): only comments, no imports. Then run: `npm uninstall cmdk`
Expected: `cmdk` gone from `package.json`.

- [ ] **Step 9: Typecheck**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 10: Commit**

```bash
git add src/components/ui/kbd.tsx src/components/ui/kbd.test.tsx src/components/CommandPalette.tsx src/components/CommandPalette.test.tsx src/test-setup.ts package.json package-lock.json
git commit -F - <<'EOF'
feat(v2): the command palette list and its key hints are XiodUI's

Ctrl+K opens XiodUI's command list in the app's colours. Typing narrows
it, Enter runs the highlighted row, and each row's shortcut shows as
XiodUI key caps, read out as words. cmdk is gone.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 6: Date picker opens XiodUI's calendar

**Files:**
- Modify (rewrite): `src/components/ui/calendar.tsx`, `src/components/ui/datefield.tsx`
- Create: `src/components/ui/datefield.test.tsx`
- Modify: `src/lib/actionIcons.ts` (add `IconPickDate`)
- Modify: `package.json`, `package-lock.json` (react-day-picker removed)

**Interfaces:**
- Consumes: `Calendar` from `xiod-ui/calendar` (`mode`, `selected: Date | DateRange | null`, `onSelect(d: Date | DateRange | undefined)`, `weekStartsOn`, `showOutsideDays`, `className`; day buttons named `"<Weekday>, <Month> <d>, <yyyy>"`; header shows `"<Month> <yyyy>"`; footer has its own "Today" button; it focuses the selected (or today's) day when it mounts).
- Produces: unchanged `export default function Calendar({ selected?: Date, onSelect: (d?: Date) => void })` and `export default function DateField({ value, onChange, ariaLabel, className })` (value `""` or `"YYYY-MM-DD"`). New icon `IconPickDate` in `src/lib/actionIcons.ts`.

- [ ] **Step 1: Write the failing tests**

Create `src/components/ui/datefield.test.tsx`:

```tsx
// DateField: the app's date input - a button showing the date that opens
// XiodUI's calendar in an inline panel. Value is "" or "YYYY-MM-DD".

import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { expect, test, vi } from "vitest";
import DateField from "./datefield";

function Host({ initial = "", onChange = () => {} }: { initial?: string; onChange?: (v: string) => void }) {
  const [v, setV] = useState(initial);
  return (
    <DateField
      ariaLabel="Target date"
      value={v}
      onChange={(next) => {
        setV(next);
        onChange(next);
      }}
    />
  );
}

const iso = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;

test("empty, it says so, and the field is named for what it holds", () => {
  render(<Host />);
  expect(screen.getByRole("button", { name: "Target date" })).toHaveTextContent("Pick a date");
});

test("it opens on the value's month, weeks starting Monday; picking a day sets it, closes, and hands focus back", () => {
  const onChange = vi.fn();
  render(<Host initial="2026-09-10" onChange={onChange} />);
  const field = screen.getByRole("button", { name: "Target date" });
  fireEvent.click(field);
  expect(screen.getByText("September 2026")).toBeInTheDocument();
  expect(screen.getAllByText(/^(Mo|Tu|We|Th|Fr|Sa|Su)$/)[0]).toHaveTextContent("Mo");

  fireEvent.click(screen.getByRole("button", { name: "Tuesday, September 15, 2026" }));
  expect(onChange).toHaveBeenLastCalledWith("2026-09-15");
  expect(screen.queryByText("September 2026")).not.toBeInTheDocument();
  // The calendar held focus and is gone: focus comes back to the field.
  expect(field).toHaveFocus();
});

test("Clear empties it", () => {
  const onChange = vi.fn();
  render(<Host initial="2026-09-10" onChange={onChange} />);
  fireEvent.click(screen.getByRole("button", { name: "Target date" }));
  fireEvent.click(screen.getByRole("button", { name: "Clear" }));
  expect(onChange).toHaveBeenLastCalledWith("");
  expect(screen.getByRole("button", { name: "Target date" })).toHaveTextContent("Pick a date");
});

test("the calendar's own Today sets today", () => {
  const onChange = vi.fn();
  render(<Host onChange={onChange} />);
  fireEvent.click(screen.getByRole("button", { name: "Target date" }));
  fireEvent.click(screen.getByRole("button", { name: "Today" }));
  expect(onChange).toHaveBeenLastCalledWith(iso(new Date()));
});

// The field lives in the work-item drawer, whose window-level Escape closes
// the drawer - and its unsaved draft. With the calendar open (and holding
// focus), Escape must close the calendar only.
test("Escape closes the calendar and nothing behind it", () => {
  const behind = vi.fn();
  window.addEventListener("keydown", behind);
  try {
    render(<Host initial="2026-09-10" />);
    const field = screen.getByRole("button", { name: "Target date" });
    fireEvent.click(field);
    fireEvent.keyDown(screen.getByRole("button", { name: "Thursday, September 10, 2026" }), { key: "Escape" });
    expect(screen.queryByText("September 2026")).not.toBeInTheDocument();
    expect(field).toHaveFocus();
    expect(behind).not.toHaveBeenCalled();
  } finally {
    window.removeEventListener("keydown", behind);
  }
});

test("a mousedown outside closes it", () => {
  render(
    <>
      <Host initial="2026-09-10" />
      <p>elsewhere</p>
    </>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Target date" }));
  fireEvent.mouseDown(screen.getByText("elsewhere"));
  expect(screen.queryByText("September 2026")).not.toBeInTheDocument();
});
```

- [ ] **Step 2: Run them to verify they fail**

Run: `npx vitest run src/components/ui/datefield.test.tsx --exclude "**/.claude/**"`
Expected: FAIL - react-day-picker's grid names its days differently (no "Tuesday, September 15, 2026" button), its caption is not "September 2026" in the same form, focus is not returned, and Escape reaches the window listener.

- [ ] **Step 3: Add the icon**

In `src/lib/actionIcons.ts`, replace

```ts
  // Everything else
  LogIn as IconSignIn,
```

with

```ts
  // Everything else
  // Opening the calendar to choose a date (DateField).
  CalendarDays as IconPickDate,
  LogIn as IconSignIn,
```

- [ ] **Step 4: Rewrite the calendar wrapper**

Replace the whole of `src/components/ui/calendar.tsx` with:

```tsx
import { Calendar as XiodCalendar } from "xiod-ui/calendar";

/**
 * The date grid DateField opens: XiodUI's calendar, weeks starting on
 * Monday, coloured by the app's tokens through src/xiod-theme.css. Single
 * dates only - that is all an Azure DevOps date field holds. Its own footer
 * carries "Today". It lays itself flat (no card of its own) because the
 * panel it sits in is the card.
 */
export default function Calendar({
  selected,
  onSelect,
}: {
  selected?: Date;
  onSelect: (d?: Date) => void;
}) {
  return (
    <XiodCalendar
      mode="single"
      selected={selected ?? null}
      onSelect={(d) => onSelect(d instanceof Date ? d : undefined)}
      weekStartsOn={1}
      showOutsideDays
      className="border-none bg-transparent shadow-none before:shadow-none!"
    />
  );
}
```

- [ ] **Step 5: Rewrite DateField**

Replace the whole of `src/components/ui/datefield.tsx` with:

```tsx
import { useEffect, useRef, useState } from "react";
import { IconPickDate } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import Calendar from "./calendar";

function toDate(v: string): Date | undefined {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(v)) return undefined;
  const [y, m, d] = v.split("-").map(Number);
  return new Date(y, m - 1, d);
}

function toIso(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/**
 * Date input backed by XiodUI's calendar instead of the native browser date
 * control. Value is "" or "YYYY-MM-DD" (what ADO fields use).
 *
 * The panel is inline (absolutely positioned under the field), not portalled:
 * the only DateField lives in the work-item drawer, whose focus trap would
 * pull focus back out of anything rendered at <body>.
 */
export default function DateField({
  value,
  onChange,
  ariaLabel,
  className,
}: {
  value: string;
  onChange: (v: string) => void;
  ariaLabel: string;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  // The calendar takes focus as it opens, so closing it hands focus back to
  // the field - otherwise it would fall to <body> with the day that held it.
  const close = () => {
    setOpen(false);
    trigger.current?.focus();
  };
  const settle = (v: string) => {
    onChange(v);
    close();
  };

  const date = toDate(value);

  return (
    <div ref={ref} className={cn("relative", className)}>
      <button
        ref={trigger}
        type="button"
        aria-label={ariaLabel}
        aria-expanded={open}
        className={cn(
          "flex w-full items-center justify-between gap-2 rounded-md border border-border bg-surface px-2 py-1.5 text-left text-sm transition-colors hover:border-border-strong focus:border-accent focus:outline-none",
          // While open, focus lives in the calendar - the field keeps the
          // accent explicitly so every dropdown shows the same lit border
          // as a focused Input.
          open && "border-accent",
        )}
        onClick={() => setOpen((o) => !o)}
      >
        <span className={date ? "text-text" : "text-faint"}>
          {date
            ? date.toLocaleDateString(undefined, { day: "2-digit", month: "short", year: "numeric" })
            : "Pick a date"}
        </span>
        <IconPickDate aria-hidden className="size-3.5 shrink-0 text-muted" />
      </button>

      {open && (
        <div
          className="absolute left-0 top-full z-40 mt-1 rounded-2xl border border-border bg-surface p-1 shadow-xl"
          onKeyDown={(e) => {
            // Close the calendar only. The drawer around it closes on a
            // window-level Escape, and would take its unsaved draft along.
            if (e.key !== "Escape") return;
            e.stopPropagation();
            close();
          }}
        >
          <Calendar selected={date} onSelect={(d) => settle(d ? toIso(d) : "")} />
          <div className="flex justify-end border-t border-border px-3 py-1.5 text-xs">
            <button type="button" className="text-muted hover:text-danger" onClick={() => settle("")}>
              Clear
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `npx vitest run src/components/ui/datefield.test.tsx src/xiod.test.ts src/ui-consistency.test.ts --exclude "**/.claude/**"`
Expected: PASS.

- [ ] **Step 7: Remove react-day-picker**

Grep (tool) pattern `react-day-picker` over `src`: no matches. Then run: `npm uninstall react-day-picker`
Expected: gone from `package.json`.

- [ ] **Step 8: Work-item drawer still behaves**

Run: `npx vitest run src/components --exclude "**/.claude/**"`
Expected: PASS.

- [ ] **Step 9: Typecheck**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 10: Commit**

```bash
git add src/components/ui/calendar.tsx src/components/ui/datefield.tsx src/components/ui/datefield.test.tsx src/lib/actionIcons.ts package.json package-lock.json
git commit -F - <<'EOF'
feat(v2): the date picker opens XiodUI's calendar

Start and target dates on a work item open XiodUI's calendar in the
app's colours, weeks starting Monday, with Today in its footer and Clear
beneath it. The calendar takes focus as it opens, hands it back to the
field when it closes, and Escape now closes just the calendar instead of
the whole work item. react-day-picker is gone.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 7: Nothing left over, README, full gates

**Files:**
- Modify: `README.md:339-348`

**Interfaces:**
- Consumes: everything above.
- Produces: nothing new.

- [ ] **Step 1: Nothing of the old three is left**

With Grep (tool): pattern `from "(sonner|cmdk|react-day-picker)"|vi\.mock\("(sonner|cmdk|react-day-picker)"` over `src` - no matches. `Read` `package.json`: none of `sonner`, `cmdk`, `react-day-picker` in `dependencies`; `xiod-ui` is `"1.0.3"`; `react`/`react-dom` are `"~19.3.0"`. Glob `node_modules/sonner/package.json`, `node_modules/cmdk/package.json`, `node_modules/react-day-picker/package.json`: none found (if one is, something still depends on it - report it rather than force-removing).

- [ ] **Step 2: README**

In `README.md`, replace

```md
- Tailwind v4 with CSS-variable design tokens. Themes are token sets keyed
  off data-theme and data-accent on the html element. A consistency test
  fails the build on a hardcoded colour.
```

with

```md
- Tailwind v4 with CSS-variable design tokens. Themes are token sets keyed
  off data-theme and data-accent on the html element. A consistency test
  fails the build on a hardcoded colour.
- Six controls come from XiodUI (`xiod-ui`, pinned to exactly 1.0.3): the
  checkbox, text area, the date picker's calendar, the command palette
  list, keyboard-key hints and toasts. Each sits behind the app's own
  wrapper in `src/components/ui/` (toasts through `src/lib/toast.ts`), and
  its colours are the app's tokens, mapped in `src/xiod-theme.css`.
  `src/xiod.test.ts` holds the pin, the vetted versions, the import
  allowlist and the licence notices (`public/THIRD-PARTY-NOTICES.txt`).
```

and replace

```md
Tauri 2, Rust (tokio, reqwest, quick-xml), tauri-specta, React 19, Vite,
TypeScript, Tailwind v4, TanStack Query, Vitest, Velopack.
```

with

```md
Tauri 2, Rust (tokio, reqwest, quick-xml), tauri-specta, React 19, Vite,
TypeScript, Tailwind v4, XiodUI (on Base UI), TanStack Query, Vitest,
Velopack.
```

- [ ] **Step 3: Full frontend suite**

Run: `npx vitest run --exclude "**/.claude/**"`
Expected: PASS (one App.test failure that passes on a re-run is its documented flake; anything else is not).

- [ ] **Step 4: Typecheck**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 5: Production build**

Run: `npm run build`
Expected: succeeds. Then Glob `dist/THIRD-PARTY-NOTICES.txt` (exists) and Grep over `dist/assets` (glob `*.css`) for the patterns `data-slot=\W?calendar`, `data-slot=\W?command-dialog-popup` and `--xiod-accent` (all present).

- [ ] **Step 6: Commit**

```bash
git add README.md
git commit -F - <<'EOF'
docs(v2): the README names XiodUI and where its colours come from

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

- [ ] **Step 7: Hand the owner the walk-through jsdom cannot do**

jsdom does no layout, painting or hit testing, so these need a person in `npm run tauri dev` (the owner runs it; do not start it on the shared machine unasked). Report this list:
1. Light, slate and one dark preset (midnight/oled), and one non-green accent: checkbox (empty, checked, mixed), a toast of each kind with the Undo action, the palette (Ctrl+K) with a highlighted row, a text area focused, the date picker in a work item - all in the theme's colours, nothing white-on-white.
2. Toasts come up bottom-right and are not hidden behind a modal; in the runner, pointing at a toast makes it leave.
3. A text area with `text-xs` (Report a bug's repro steps) reads small; a text area drags taller by its corner.
4. The date picker panel sits under its field inside the drawer and is not clipped; Tab from the calendar stays in the drawer; Escape closes only the calendar.
5. Windows "Show animations" off: the checkbox tick, toasts and palette do not animate.

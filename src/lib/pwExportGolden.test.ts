// Every golden raw spec the Playwright export is pinned to (the Rust suite
// keeps each byte-for-byte what the translator writes today) must be
// TypeScript that compiles: an undeclared name or a syntax error is a spec
// that fails before its first step, however right each line looks alone.
//
// `@playwright/test` is declared as `any`, so this proves names and syntax,
// not Playwright's own signatures. That `any` also makes every callback
// parameter implicitly `any`, so noImplicitAny is off; the rest of strict
// stays on (noImplicitThis is what keeps the text reader's `this` typed).

import fs from "node:fs";
import path from "node:path";
import ts from "typescript";
import { expect, test } from "vitest";

const DIR = path.resolve(__dirname, "../../src-tauri/tests/fixtures/pw_export");
const AMBIENT = "/virtual/playwright.d.ts";
const AMBIENT_TEXT = "declare module '@playwright/test' { export const test: any; export const expect: any; }\n";

const OPTIONS: ts.CompilerOptions = {
  noEmit: true,
  strict: true,
  noImplicitAny: false,
  target: ts.ScriptTarget.ES2022,
  module: ts.ModuleKind.ESNext,
  moduleResolution: ts.ModuleResolutionKind.Bundler,
  lib: ["lib.es2022.d.ts", "lib.dom.d.ts", "lib.dom.iterable.d.ts"],
  types: [],
};

/** The compiler's own lib files, parsed once: lib.dom alone takes seconds. */
const libCache = new Map<string, ts.SourceFile>();

/** Each program parses lib.dom; on a busy machine that outlives the default 5 s. */
const SLOW = 60_000;

/** Error diagnostics for one spec, as `TS<code>: <message>` lines. */
function check(name: string, source: string): string[] {
  const file = `/virtual/${name}.ts`;
  const files = new Map([
    [file, source],
    [AMBIENT, AMBIENT_TEXT],
  ]);
  const libDir = path.dirname(ts.getDefaultLibFilePath(OPTIONS));
  const isLib = (f: string) => path.resolve(f).startsWith(path.resolve(libDir));
  const host: ts.CompilerHost = {
    getSourceFile: (f, lang) => {
      const own = files.get(f);
      if (own !== undefined) return ts.createSourceFile(f, own, lang);
      if (!isLib(f)) return undefined;
      let lib = libCache.get(f);
      if (!lib) {
        const text = ts.sys.readFile(f);
        if (text === undefined) return undefined;
        lib = ts.createSourceFile(f, text, lang);
        libCache.set(f, lib);
      }
      return lib;
    },
    getDefaultLibFileName: (o) => ts.getDefaultLibFilePath(o),
    writeFile: () => {},
    getCurrentDirectory: () => "/virtual",
    getCanonicalFileName: (f) => f,
    useCaseSensitiveFileNames: () => true,
    getNewLine: () => "\n",
    // Nothing on disk but the compiler's own libs: '@playwright/test' must
    // come from the ambient declaration, never a copy in node_modules.
    fileExists: (f) => files.has(f) || (isLib(f) && ts.sys.fileExists(f)),
    readFile: (f) => files.get(f) ?? (isLib(f) ? ts.sys.readFile(f) : undefined),
    directoryExists: () => false,
    getDirectories: () => [],
  };
  const program = ts.createProgram([file, AMBIENT], OPTIONS, host);
  return ts
    .getPreEmitDiagnostics(program)
    .filter((d) => d.category === ts.DiagnosticCategory.Error)
    .map((d) => `TS${d.code}: ${ts.flattenDiagnosticMessageText(d.messageText, "\n")}`);
}

const goldens = fs.readdirSync(DIR).filter((f) => f.endsWith(".spec.ts.golden"));

test("the goldens are all here", () => {
  expect(goldens).toEqual(
    expect.arrayContaining(["raw-135560.spec.ts.golden", "kitchen-sink.spec.ts.golden", "open-tab-only.spec.ts.golden"]),
  );
});

test.each(goldens)("%s type-checks", { timeout: SLOW }, (name) => {
  expect(check(name, fs.readFileSync(path.join(DIR, name), "utf8"))).toEqual([]);
});

test("the check itself catches an undeclared name and a syntax error", { timeout: SLOW }, () => {
  const head = "import { test, expect } from '@playwright/test';\ntest('x', async ({ page }) => {\n";
  expect(check("undeclared", `${head}  claimed.add(page);\n});\n`).join("\n")).toMatch(/TS2304: Cannot find name 'claimed'/);
  expect(check("syntax", `${head}  await page.goto('/';\n});\n`).length).toBeGreaterThan(0);
  expect(check("this", `${head}  const f = function () { return this.x; };\n});\n`).join("\n")).toMatch(/TS2683/);
});

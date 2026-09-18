/**
 * The review page's spec-pane helpers live in a plain browser script
 * embedded in the page (src-tauri/web/cases-specs.js). The pure ones sit on
 * window.tcmSpecs; this loads the file as-is and exercises them.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeAll, describe, expect, test } from "vitest";

type Doc = { title: string; kind: string; source: string };
type Helpers = {
  splitCitation: (text: string) => { document: string; section: string } | null;
  findSpecTab: (docs: Doc[], document: string) => number;
  matchHeading: (headings: string[], section: string) => number;
  slug: (text: string) => string;
};
let H: Helpers;

beforeAll(() => {
  // import.meta.url, not `__dirname`: this file is ESM under vitest.
  const here = dirname(fileURLToPath(import.meta.url));
  const src = readFileSync(resolve(here, "../../src-tauri/web/cases-specs.js"), "utf8");
  new Function(src)();
  H = (window as unknown as { tcmSpecs: Helpers }).tcmSpecs;
});

describe("splitCitation", () => {
  test("splits the document (up to its extension) from the section, dropping quote and exemption", () => {
    expect(H.splitCitation("Spec: Step13-CalculationEngine.md 5.8 Display Rules")).toEqual({
      document: "Step13-CalculationEngine.md",
      section: "5.8 Display Rules",
    });
    expect(H.splitCitation("Spec: Step 10 Manage Cycle.md 7.7 (AC-3) > \"quoted\"")).toEqual({
      document: "Step 10 Manage Cycle.md",
      section: "7.7 (AC-3)",
    });
    expect(H.splitCitation("Spec: Engine 5.8 Display Rules - no quotable text (table)")).toEqual({
      document: "Engine",
      section: "5.8 Display Rules",
    });
    expect(H.splitCitation("  spec:   Rules.md   ")).toEqual({ document: "Rules.md", section: "" });
    expect(H.splitCitation("Code: foo()")).toBeNull();
  });
});

describe("findSpecTab", () => {
  const docs: Doc[] = [
    { title: "Calculation Engine", kind: "file", source: "C:/s/Step13-CalculationEngine.md" },
    { title: "Display Rules", kind: "wiki", source: "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Display-Rules" },
  ];
  test("matches by file name or title, ignoring case and extension, then by containment", () => {
    expect(H.findSpecTab(docs, "Step13-CalculationEngine.md")).toBe(0);
    expect(H.findSpecTab(docs, "step13-calculationengine")).toBe(0);
    expect(H.findSpecTab(docs, "Display Rules")).toBe(1);
    expect(H.findSpecTab(docs, "Display-Rules")).toBe(1);
    expect(H.findSpecTab(docs, "CalculationEngine")).toBe(0);
    expect(H.findSpecTab(docs, "Nothing.md")).toBe(-1);
  });
});

describe("matchHeading", () => {
  const headings = ["1 Overview", "5.8 Display Rules", "5.80 Other", "5.8.1 Flags", "Appendix"];
  test("prefers the exact leading number, then word overlap", () => {
    expect(H.matchHeading(headings, "5.8 Display Rules")).toBe(1);
    expect(H.matchHeading(headings, "5.8")).toBe(1);
    expect(H.matchHeading(headings, "5.8.1")).toBe(3);
    expect(H.matchHeading(headings, "display rules")).toBe(1);
    expect(H.matchHeading(headings, "Appendix")).toBe(4);
    expect(H.matchHeading(headings, "9.9 Missing section")).toBe(-1);
    expect(H.matchHeading(headings, "")).toBe(-1);
  });
});

test("slug is stable, lowercase, and safe for an id", () => {
  expect(H.slug("5.8 Display Rules")).toBe("5-8-display-rules");
  expect(H.slug("  Ünïcode & symbols!  ")).toBe("ünïcode-symbols");
});

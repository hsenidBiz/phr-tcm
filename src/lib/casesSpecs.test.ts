/**
 * The review page's spec-pane helpers live in a plain browser script
 * embedded in the page (src-tauri/web/cases-specs.js). The pure ones sit on
 * window.tcmSpecs; this loads the file as-is and exercises them.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeAll, describe, expect, test, vi } from "vitest";

type Doc = { title: string; kind: string; source: string };
type Helpers = {
  splitCitation: (text: string) => { document: string; section: string } | null;
  findSpecTab: (docs: Doc[], document: string) => number;
  matchHeading: (headings: string[], section: string) => number;
  slug: (text: string) => string;
  citationStart: (text: string) => number;
  scrollWithin: (container: Element, target: Element, margin: number) => void;
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

  test("with no extension anywhere, the whole tail is the section - just its first word is the document", () => {
    expect(H.splitCitation("Spec: Calculation Engine 5.8 Display Rules")).toEqual({
      document: "Calculation",
      section: "Engine 5.8 Display Rules",
    });
  });
});

describe("citationStart", () => {
  test("finds the Spec: token preceded by start-of-text or whitespace, else -1", () => {
    expect(H.citationStart("Spec: A.md 1")).toBe(0);
    const text = "Checks it. Spec: A.md 1";
    expect(H.citationStart(text)).toBe(text.indexOf("Spec:"));
    expect(H.citationStart("Respect: none")).toBe(-1);
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

  test("picks the longer, space-insensitive match over a shorter one contained in it", () => {
    const camel: Doc[] = [
      { title: "Calculation Engine", kind: "file", source: "C:/s/Rules.md" },
      { title: "Engine", kind: "wiki", source: "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine" },
    ];
    expect(H.findSpecTab(camel, "Step13-CalculationEngine.md")).toBe(0);
    expect(H.findSpecTab(camel, "Engine")).toBe(1);
    expect(H.findSpecTab(camel, "calculationengine")).toBe(0);
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

describe("scrollWithin", () => {
  test("moves only the container, by the target's offset inside it less the margin", () => {
    const container = document.createElement("div");
    const target = document.createElement("h2");
    container.appendChild(target);
    let top = 100;
    Object.defineProperty(container, "scrollTop", { get: () => top, set: (v: number) => { top = v; }, configurable: true });
    container.getBoundingClientRect = () => ({ top: 40 } as DOMRect);
    target.getBoundingClientRect = () => ({ top: 400 } as DOMRect);
    const scrolled = vi.spyOn(window, "scrollTo").mockImplementation(() => {});
    const into = vi.spyOn(Element.prototype, "scrollIntoView").mockImplementation(() => {});
    (H as unknown as { scrollWithin: (c: Element, t: Element, m: number) => void }).scrollWithin(container, target, 12);
    expect(top).toBe(100 + 400 - 40 - 12);
    expect(scrolled).not.toHaveBeenCalled();
    expect(into).not.toHaveBeenCalled();
    scrolled.mockRestore();
    into.mockRestore();
  });
});

describe("a spec citation link on the review page", () => {
  test("clicking it scrolls the spec article only, never the window via scrollIntoView", () => {
    document.body.innerHTML = `
      <div class="shell with-specs">
        <div class="rev">Spec: Rules.md 2 Login</div>
        <section id="tc-specs">
          <button type="button" class="spec-tab" data-spec="0">Rules.md</button>
          <article class="spec-doc" data-spec="0"><h2>2 Login</h2></article>
        </section>
      </div>
      <script type="application/json" id="tc-specs-data">[{"title":"Rules.md","kind":"file","source":"x"}]</script>
    `;

    const article = document.querySelector(".spec-doc") as HTMLElement;
    let top = 0;
    Object.defineProperty(article, "scrollTop", {
      get: () => top,
      set: (v: number) => { top = v; },
      configurable: true,
    });
    article.getBoundingClientRect = () => ({ top: 0 } as DOMRect);
    const heading = article.querySelector("h2") as HTMLElement;
    heading.getBoundingClientRect = () => ({ top: 200 } as DOMRect);

    const into = vi.spyOn(Element.prototype, "scrollIntoView").mockImplementation(() => {});

    (window as unknown as { __tcmWireSpecs: () => void }).__tcmWireSpecs();

    const link = document.querySelector("a.spec-link") as HTMLAnchorElement;
    expect(link).not.toBeNull();
    link.click();

    expect(into).not.toHaveBeenCalled();
    expect(top).not.toBe(0);

    into.mockRestore();
  });
});

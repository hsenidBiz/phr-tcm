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
  splitCitation: (text: string, titles?: string[]) => { document: string; section: string } | null;
  findSpecTab: (docs: Doc[], document: string) => number;
  matchHeading: (headings: string[], section: string) => number;
  slug: (text: string) => string;
  citationStart: (text: string) => number;
  scrollWithin: (container: Element, target: Element, margin: number) => void;
  citationRanges: (text: string) => Array<{ start: number; end: number }>;
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

describe("splitCitation with known titles", () => {
  test("a known multi-word title without an extension is the document, longest first", () => {
    expect(H.splitCitation("Spec: Calculation Engine 5.8 Display Rules", ["Engine", "Calculation Engine"])).toEqual({
      document: "Calculation Engine",
      section: "5.8 Display Rules",
    });
    expect(H.splitCitation("Spec: calculation   engine 5.8", ["Calculation Engine"])).toEqual({
      document: "calculation engine",
      section: "5.8",
    });
  });
  test("a title only counts when it ends at a word boundary", () => {
    expect(H.splitCitation("Spec: Calculation Engine 5.8", ["Calc"])).toEqual({
      document: "Calculation",
      section: "Engine 5.8",
    });
    expect(H.splitCitation("Spec: A.md 5.8", ["A"])).toEqual({ document: "A.md", section: "5.8" });
  });
});

describe("citationRanges", () => {
  test("finds every citation in one run of text, each ending where the next starts", () => {
    const t = "Checks it. Spec: A.md 5.8\nSpec: B.md 2.1 ";
    expect(H.citationRanges(t).map((r) => t.slice(r.start, r.end))).toEqual(["Spec: A.md 5.8", "Spec: B.md 2.1"]);
    expect(H.citationRanges("Respect: none")).toEqual([]);
  });
});

test("a heading in a script without letter case keeps its words in the slug", () => {
  expect(H.slug("5.8 表示ルール")).toBe("5-8-表示ルール");
  expect(H.slug("ログイン　画面")).toBe("ログイン-画面"); // an ideographic space separates
  expect(H.slug("概要、目的。")).toBe("概要-目的");
  expect(H.slug("사용자 설정")).toBe("사용자-설정");
  expect(H.slug("🙂 Emoji")).toBe("emoji");
  // Nothing wordlike at all: empty, and the page falls back to "h".
  expect(H.slug("🙂 !!")).toBe("");
});

/// Citations resolve through matchHeading, not through the ids, so a
/// caseless heading must still be found by its words and by its number.
test("a citation still finds a heading written without letter case", () => {
  const headings = ["1 概要", "2 表示ルール", "3 ログイン 画面"];
  expect(H.matchHeading(headings, "2 表示ルール")).toBe(1);
  expect(H.matchHeading(headings, "表示ルール")).toBe(1);
  expect(H.matchHeading(headings, "ログイン 画面")).toBe(2);
});

// Review round 1: the slug switched from a hand-picked block list to the
// Unicode L/N/M class (built with \p{} at runtime). These pin that the
// switch changed nothing for the ids already in the wild.
test("ASCII and accented-Latin slugs are unchanged by the switch to \\p{L}\\p{N}\\p{M}", () => {
  expect(H.slug("5.8 Display Rules")).toBe("5-8-display-rules");
  expect(H.slug("Café résumé naïve")).toBe("café-résumé-naïve");
  expect(H.slug("Überblick – Ärger")).toBe("überblick-ärger");
});

/// \p{M} alone would let an emoji's variation selector (U+FE0F, category
/// Mn) start a "word" of its own - invisible, and first in the id. A mark
/// must only continue a word already open, the way a combining accent
/// decorates the letter right before it.
test("a bare mark such as an emoji's variation selector never starts a word", () => {
  expect(H.slug("⚠️ Warning")).toBe("warning");
});

/// The hand-picked block list dropped these as punctuation/symbols; \p{N}
/// and \p{L} correctly keep them.
test("a heading with a fraction or a letterlike symbol keeps that character", () => {
  expect(H.slug("Add 1½ cups")).toBe("add-1½-cups");
  expect(H.slug("Set ℂ")).toBe("set-ℂ");
});

describe("citation links in the page", () => {
  test("every citation in a paragraph is linked, and a link is never nested in a link", () => {
    document.body.innerHTML = `
      <section id="tc-specs"><div class="spec-tabs">
        <button class="spec-tab" data-spec="0">A</button><button class="spec-tab" data-spec="1">B</button></div>
        <article class="spec-doc" data-spec="0"><h2>5.8 Rules</h2></article>
        <article class="spec-doc" data-spec="1"><h2>2.1 Flags</h2></article>
        <script type="application/json" id="tc-specs-data">[{"title":"A","kind":"file","source":"C:/s/A.md"},{"title":"B","kind":"file","source":"C:/s/B.md"}]</script>
      </section>
      <details class="rev" open><summary>Reviewer notes</summary><div class="rev-body">
        <p id="para">Checks it. Spec: A.md 5.8 Spec: B.md 2.1</p>
        <p><a id="md-link" href="https://example.test/">Spec: A.md 5.8</a></p>
      </div></details>`;
    (window as unknown as { __tcmWireSpecs: () => void }).__tcmWireSpecs();
    const links = Array.from(document.querySelectorAll("#para a.spec-link"));
    expect(links.map((a) => a.textContent)).toEqual(["Spec: A.md 5.8", "Spec: B.md 2.1"]);
    expect(links.map((a) => a.getAttribute("data-spec"))).toEqual(["0", "1"]);
    expect(document.getElementById("para")!.textContent).toBe("Checks it. Spec: A.md 5.8 Spec: B.md 2.1");
    expect(document.querySelector("#md-link a")).toBeNull();
  });
});

/**
 * The review page's bookmark and its Options menu live in a plain browser
 * script embedded in the page (src-tauri/web/cases-page.js). This loads the
 * file as-is into a jsdom page shaped like the real one and drives it.
 *
 * The mark is browser-only by design: it is written to the page's own
 * storage under the scope the renderer put on <body>, and never reaches the
 * app.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeAll, beforeEach, describe, expect, test, vi } from "vitest";

const SCOPE = "pbi-42";
const STORE = `tcm-report-mark:${SCOPE}`;

function page(): string {
  return `
    <div class='page'>
      <div class='searchbar'>
        <input id='tc-search' type='search'>
        <button id='tc-goto' type='button' class='hidden'>Go to bookmark</button>
        <details class='tc-menu'><summary>Options</summary><div class='tc-menu-items'>
          <button id='tc-notes' type='button' aria-pressed='false'>Hide reviewer notes</button>
        </div></details>
        <span id='tc-count'></span>
      </div>
      <div class='case' data-key='101'><h2>One<button type='button' class='tc-mark' aria-pressed='false'></button></h2></div>
      <div class='case' data-key='102'><h2>Two<button type='button' class='tc-mark' aria-pressed='false'></button></h2></div>
      <div class='case' data-key='d0'><h2>Three<button type='button' class='tc-mark' aria-pressed='false'></button></h2></div>
    </div>`;
}

function cases(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>(".case"));
}

function mark(index: number): void {
  cases()[index].querySelector<HTMLButtonElement>(".tc-mark")!.click();
}

function rewire(): void {
  (window as unknown as { __tcmWireMarks: () => void }).__tcmWireMarks();
}

/** The script is loaded once in `beforeAll` and reused for every test in
 * this file, so its in-memory write-through cache (`sessionMark`) has to be
 * reset by hand between tests - on a real page it lasts only as long as
 * the page itself, which is exactly one test here. */
function forgetSessionMark(): void {
  (window as unknown as { __tcmForgetMark: () => void }).__tcmForgetMark();
}

beforeAll(() => {
  // import.meta.url, not `__dirname`: this file is ESM under vitest.
  const here = dirname(fileURLToPath(import.meta.url));
  const src = readFileSync(resolve(here, "../../src-tauri/web/cases-page.js"), "utf8");
  document.body.setAttribute("data-scope", SCOPE);
  document.body.innerHTML = page();
  // Once: the script attaches its delegated listeners to `document`, which
  // survives the body being rebuilt - exactly as it survives the page's own
  // live content swap.
  new Function(src)();
});

beforeEach(() => {
  localStorage.clear();
  forgetSessionMark();
  document.body.innerHTML = page();
  rewire();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("the bookmark", () => {
  test("marks one case and stores its key under this page's scope", () => {
    mark(1);
    expect(localStorage.getItem(STORE)).toBe("102");
    expect(cases()[1].classList.contains("marked")).toBe(true);
    expect(cases()[1].querySelector(".tc-mark")!.getAttribute("aria-pressed")).toBe("true");
    expect(cases()[0].classList.contains("marked")).toBe(false);
    expect(cases()[2].classList.contains("marked")).toBe(false);
  });

  test("marking another case moves the one mark", () => {
    mark(1);
    mark(2);
    expect(localStorage.getItem(STORE)).toBe("d0");
    expect(cases()[1].classList.contains("marked")).toBe(false);
    expect(cases()[2].classList.contains("marked")).toBe(true);
  });

  test("clicking the marked case again clears it", () => {
    mark(0);
    mark(0);
    expect(localStorage.getItem(STORE)).toBeNull();
    expect(cases()[0].classList.contains("marked")).toBe(false);
    expect(document.getElementById("tc-goto")!.classList.contains("hidden")).toBe(true);
  });

  test("survives the live swap: re-wiring repaints the mark on the new cards", () => {
    mark(1);
    document.body.innerHTML = page();
    rewire();
    expect(cases()[1].classList.contains("marked")).toBe(true);
  });

  test("Go to bookmark appears once something is marked, and scrolls to it", () => {
    const go = document.getElementById("tc-goto")!;
    expect(go.classList.contains("hidden")).toBe(true);
    mark(2);
    expect(go.classList.contains("hidden")).toBe(false);
    // jsdom does no layout and has no scrollIntoView; the page guards the
    // call, so a stub is what proves it reached the right card.
    const scrolled = vi.fn();
    cases()[2].scrollIntoView = scrolled;
    go.click();
    expect(scrolled).toHaveBeenCalled();
  });

  test("a write that silently fails (quota, Safari private mode) still gets a mark for the session", () => {
    // Reads keep working; only the write is refused - unlike the "origin
    // refuses storage outright" case above, where both throw. `getItem`
    // has to genuinely run and come back with null (nothing was ever
    // written) rather than throwing, or this reproduces the wrong bug.
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota exceeded");
    });
    mark(0);
    expect(localStorage.getItem(STORE)).toBeNull(); // the write really did fail
    expect(cases()[0].classList.contains("marked")).toBe(true);
    const go = document.getElementById("tc-goto")!;
    expect(go.classList.contains("hidden")).toBe(false);
    const scrolled = vi.fn();
    cases()[0].scrollIntoView = scrolled;
    go.click();
    expect(scrolled).toHaveBeenCalled();
  });

  test("an origin that refuses storage still gets a mark for the session", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("storage is not available");
    });
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("storage is not available");
    });
    expect(() => mark(0)).not.toThrow();
    expect(cases()[0].classList.contains("marked")).toBe(true);
    expect(document.getElementById("tc-goto")!.classList.contains("hidden")).toBe(false);
    // And it still moves, held for the life of the tab rather than on disk.
    mark(1);
    expect(cases()[0].classList.contains("marked")).toBe(false);
    expect(cases()[1].classList.contains("marked")).toBe(true);
  });
});

describe("the Options menu", () => {
  test("closes on Escape and on a click outside, and stays open inside", () => {
    const menu = document.querySelector<HTMLDetailsElement>(".tc-menu")!;
    menu.open = true;
    document.getElementById("tc-notes")!.click();
    expect(menu.open).toBe(true);

    document.body.click();
    expect(menu.open).toBe(false);

    menu.open = true;
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(menu.open).toBe(false);
  });
});

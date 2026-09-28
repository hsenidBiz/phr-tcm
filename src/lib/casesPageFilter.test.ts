/**
 * The review page's search - the field picker, the match switches, the
 * highlighting - and its Show on cards switches live in a plain browser
 * script embedded in the page (src-tauri/web/cases-page.js). This loads the
 * file as-is into a jsdom page shaped like the real one and drives it, the
 * same way src/lib/casesPageMarks.test.ts drives the bookmark.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeAll, beforeEach, expect, test } from "vitest";

function page(): string {
  return `
<div class="page">
  <div class="searchbar">
    <div class="tc-searchbox">
      <select id="tc-field" aria-label="Search in">
        <option value="all">All fields</option><option value="title">Title</option><option value="id">ID</option>
        <option value="pre">Prerequisites</option><option value="steps">Steps</option><option value="tags">Tags</option>
        <option value="module">Module</option>
      </select>
      <input id="tc-search" type="search">
      <span class="tc-opts"><button type="button" id="tc-case" aria-pressed="false">Aa</button><button type="button" id="tc-word" aria-pressed="false">ab</button><button type="button" id="tc-regex" aria-pressed="false">.*</button></span>
    </div>
    <details class="tc-menu"><summary>Options</summary><div class="tc-menu-items">
      <label class="tc-check"><input type="checkbox" id="tc-show-module" checked>Module</label>
      <label class="tc-check"><input type="checkbox" id="tc-show-tags" checked>Tags</label>
    </div></details>
    <span id="tc-count"></span>
  </div>
  <p id="tc-no-match" class="no-match hidden">No test cases match your search.</p>
  <div class="case" data-key="101"><h2><span class="seq">1</span><span class="wid">#101</span><span class="title">Login works</span></h2>
    <div class="meta"><div class="metarow m-module"><span class="metalabel">Module</span><span class="chip module">Auth</span></div>
    <div class="metarow m-tags"><span class="metalabel">Tags</span><span class="chip tag">smoke</span></div></div>
    <p class="pre"><b>Prerequisites:</b> A registered user</p>
    <table><tr><td class="num">1</td><td class="action">Open the page</td><td class="expected">The form shows</td></tr></table></div>
  <div class="case" data-key="d1"><h2><span class="seq">2</span><span class="title">Password reset</span></h2>
    <div class="meta"><div class="metarow m-tags"><span class="metalabel">Tags</span><span class="chip tag">regression</span></div></div>
    <p class="pre"><b>Prerequisites:</b> <span class="none">None</span></p>
    <table><tr><td class="num">1</td><td class="action">Click Forgot password</td><td class="expected">A login email arrives</td></tr></table></div>
</div>`;
}

type PageHooks = { __tcmWireSearch: () => void; __tcmWireShow: () => void };
const hooks = window as unknown as PageHooks;

const shown = () =>
  Array.from(document.querySelectorAll(".case"))
    .filter((c) => !c.classList.contains("hidden"))
    .map((c) => c.getAttribute("data-key"));

const hits = () => Array.from(document.querySelectorAll("mark.tc-hit")).map((m) => m.textContent);

const input = () => document.getElementById("tc-search") as HTMLInputElement;

function search(field: string, text: string) {
  const sel = document.getElementById("tc-field") as HTMLSelectElement;
  sel.value = field;
  sel.dispatchEvent(new Event("change"));
  input().value = text;
  input().dispatchEvent(new Event("input"));
}

/** Press a match switch until it reads `on`. */
function setOpt(id: string, on: boolean) {
  const b = document.getElementById(id)!;
  if ((b.getAttribute("aria-pressed") === "true") !== on) b.click();
}

beforeAll(() => {
  // import.meta.url, not `__dirname`: this file is ESM under vitest.
  const here = dirname(fileURLToPath(import.meta.url));
  const src = readFileSync(resolve(here, "../../src-tauri/web/cases-page.js"), "utf8");
  // Once: the script attaches its delegated listeners to `document`, which
  // survives the body being rebuilt - exactly as it survives the page's own
  // live content swap.
  new Function(src)();
});

beforeEach(() => {
  localStorage.clear();
  document.body.className = "";
  document.body.innerHTML = page();
  hooks.__tcmWireSearch();
  hooks.__tcmWireShow();
});

// The switches are the script's own state, which outlives a test's page.
afterEach(() => {
  for (const id of ["tc-case", "tc-word", "tc-regex"]) if (document.getElementById(id)) setOpt(id, false);
});

test("all fields matches anywhere; a field narrows to that field", () => {
  search("all", "login");
  expect(shown()).toEqual(["101", "d1"]); // title of one, a step of the other
  search("title", "login");
  expect(shown()).toEqual(["101"]);
  search("steps", "login");
  expect(shown()).toEqual(["d1"]);
  search("pre", "registered");
  expect(shown()).toEqual(["101"]);
  search("tags", "smoke");
  expect(shown()).toEqual(["101"]);
  search("module", "auth");
  expect(shown()).toEqual(["101"]);
  search("id", "#101");
  expect(shown()).toEqual(["101"]);
  search("id", "d1");
  expect(shown()).toEqual(["d1"]);
});

test("all fields searches what a case says, not the page's labels", () => {
  search("all", "Prerequisites");
  expect(shown()).toEqual([]);
  search("all", "module");
  expect(shown()).toEqual([]);
});

test("the count and the no-match line follow the narrowed field", () => {
  search("title", "reset");
  expect(document.getElementById("tc-count")!.textContent).toBe("1 of 2 shown");
  search("tags", "reset");
  expect(shown()).toEqual([]);
  expect(document.getElementById("tc-no-match")!.classList.contains("hidden")).toBe(false);
});

test("Escape clears the text but keeps the field", () => {
  search("title", "reset");
  input().dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  expect(input().value).toBe("");
  expect((document.getElementById("tc-field") as HTMLSelectElement).value).toBe("title");
  expect(shown()).toEqual(["101", "d1"]);
});

test("a page without the select searches every field", () => {
  // Rebuild the DOM without the select before re-wiring, rather than
  // removing it from the already-wired page: #tc-search is the same node
  // either way (dataset.wired stays set), so re-wiring alone would leave
  // the old `apply` - the one still closing over the detached select -
  // running, and this test would pass even if the missing-select guard
  // in cases-page.js's `apply()` were broken.
  document.body.innerHTML = page().replace(/<select[\s\S]*?<\/select>/, "");
  hooks.__tcmWireSearch();
  input().value = "login";
  input().dispatchEvent(new Event("input"));
  expect(shown()).toEqual(["101", "d1"]);
});

test("Match case tells Login from login", () => {
  setOpt("tc-case", true);
  search("all", "Login");
  expect(shown()).toEqual(["101"]);
  search("all", "login");
  expect(shown()).toEqual(["d1"]);
});

test("Match whole word needs the whole word", () => {
  search("title", "log");
  expect(shown()).toEqual(["101"]);
  setOpt("tc-word", true);
  expect(shown()).toEqual([]);
  search("title", "login");
  expect(shown()).toEqual(["101"]);
});

test("a quoted phrase must appear as written; loose words need only all appear", () => {
  search("all", "email login");
  expect(shown()).toEqual(["d1"]);
  search("all", '"email login"');
  expect(shown()).toEqual([]);
  search("all", '"login email"');
  expect(shown()).toEqual(["d1"]);
});

test("a regular expression searches as a pattern, and a broken one says so", () => {
  setOpt("tc-regex", true);
  search("title", "^(login|password)");
  expect(shown()).toEqual(["101", "d1"]);
  search("title", "works$");
  expect(shown()).toEqual(["101"]);
  // Still being typed: the cards stay as the last good pattern left them.
  search("title", "(works");
  expect(shown()).toEqual(["101"]);
  expect(document.getElementById("tc-count")!.textContent).toBe("Invalid regular expression");
  expect(input().getAttribute("aria-invalid")).toBe("true");
  expect(document.querySelector(".tc-searchbox")!.classList.contains("invalid")).toBe(true);
  search("title", "(works)");
  expect(input().hasAttribute("aria-invalid")).toBe(false);
});

test("Alt+C, Alt+W and Alt+R flip the switches from the search box", () => {
  input().dispatchEvent(new KeyboardEvent("keydown", { key: "c", altKey: true }));
  input().dispatchEvent(new KeyboardEvent("keydown", { key: "w", altKey: true }));
  input().dispatchEvent(new KeyboardEvent("keydown", { key: "r", altKey: true }));
  for (const id of ["tc-case", "tc-word", "tc-regex"]) {
    expect(document.getElementById(id)!.getAttribute("aria-pressed"), id).toBe("true");
  }
});

test("matches are marked in the shown cards, only in the searched field, and cleared after", () => {
  search("all", "login");
  expect(hits()).toEqual(["Login", "login"]);
  search("steps", "login");
  expect(hits()).toEqual(["login"]);
  expect(document.querySelector(".title mark")).toBeNull();
  // The labels are never marked, and the text reads the same with marks in.
  search("all", "Auth");
  expect(document.querySelector(".metalabel mark")).toBeNull();
  expect(document.querySelector(".chip.module")!.textContent).toBe("Auth");
  search("all", "");
  expect(hits()).toEqual([]);
  expect(document.querySelector(".title")!.childNodes).toHaveLength(1);
});

test("the field picker is a themed list over the select", () => {
  const btn = document.querySelector(".tc-pick-btn") as HTMLButtonElement;
  const list = document.getElementById("tc-field-list")!;
  expect((document.getElementById("tc-field") as HTMLSelectElement).hidden).toBe(true);
  expect(btn.getAttribute("aria-label")).toBe("Search in: All fields");
  expect(list.hidden).toBe(true);

  btn.click();
  expect(list.hidden).toBe(false);
  expect(btn.getAttribute("aria-expanded")).toBe("true");
  expect(document.activeElement?.getAttribute("data-value")).toBe("all");

  // Down to Title and choose it from the keyboard.
  list.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  expect(document.activeElement?.getAttribute("data-value")).toBe("title");
  list.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  expect(list.hidden).toBe(true);
  expect((document.getElementById("tc-field") as HTMLSelectElement).value).toBe("title");
  expect(btn.getAttribute("aria-label")).toBe("Search in: Title");
  expect(document.activeElement).toBe(input());

  // And it searches that field: a step's "login" no longer counts.
  input().value = "login";
  input().dispatchEvent(new Event("input"));
  expect(shown()).toEqual(["101"]);

  // Escape closes it without choosing; a click elsewhere closes it too.
  btn.click();
  list.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  expect(list.hidden).toBe(true);
  expect(document.activeElement).toBe(btn);
  btn.click();
  document.querySelector(".case")!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  expect(list.hidden).toBe(true);
});

test("Show on cards hides a kind of row, and a card with none left loses the block", () => {
  const tags = document.getElementById("tc-show-tags") as HTMLInputElement;
  tags.click();
  expect(document.body.classList.contains("hide-tags")).toBe(true);
  const [first, second] = Array.from(document.querySelectorAll(".case > .meta"));
  expect(first.classList.contains("hidden")).toBe(false); // its Module row is still shown
  expect(second.classList.contains("hidden")).toBe(true); // Tags was all it had
  expect(localStorage.getItem("tcm-report-hide-tags")).toBe("1");

  tags.click();
  expect(second.classList.contains("hidden")).toBe(false);
});

test("Show on cards is remembered for the next page", () => {
  localStorage.setItem("tcm-report-hide-module", "1");
  document.body.className = "";
  document.body.innerHTML = page();
  hooks.__tcmWireShow();
  expect(document.body.classList.contains("hide-module")).toBe(true);
  expect((document.getElementById("tc-show-module") as HTMLInputElement).checked).toBe(false);
});

// The swap() path that captures and restores the field's value only runs
// with the live-report globals (REPORT_REV / NOTE_PORT) wired in
// cases-page.js's outer IIFE at load time, the same guard casesPageMarks.test.ts
// works around by never exercising it. It was verified by reading the code
// (see the report for the exact lines) rather than by a unit here.

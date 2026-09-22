/**
 * The review page's search field selector lives in a plain browser script
 * embedded in the page (src-tauri/web/cases-page.js). This loads the file
 * as-is into a jsdom page shaped like the real one and drives it, the same
 * way src/lib/casesPageMarks.test.ts drives the bookmark.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeAll, beforeEach, expect, test } from "vitest";

function page(): string {
  return `
<div class="page">
  <div class="searchbar">
    <select id="tc-field" aria-label="Search in">
      <option value="all">All fields</option><option value="title">Title</option><option value="id">ID</option>
      <option value="pre">Prerequisites</option><option value="steps">Steps</option><option value="tags">Tags</option>
      <option value="module">Module</option>
    </select>
    <input id="tc-search" type="search"><span id="tc-count"></span>
  </div>
  <p id="tc-no-match" class="no-match hidden">No test cases match your search.</p>
  <div class="case" data-key="101"><h2><span class="seq">1</span><span class="wid">#101</span><span class="title">Login works</span></h2>
    <div class="metarow"><span class="metalabel">Module</span><span class="chip module">Auth</span></div>
    <div class="metarow"><span class="metalabel">Tags</span><span class="chip tag">smoke</span></div>
    <p class="pre"><b>Prerequisites:</b> A registered user</p>
    <table><tr><td class="num">1</td><td class="action">Open the page</td><td class="expected">The form shows</td></tr></table></div>
  <div class="case" data-key="d1"><h2><span class="seq">2</span><span class="title">Password reset</span></h2>
    <div class="metarow"><span class="metalabel">Module</span><span class="chip module">Auth</span></div>
    <p class="pre"><b>Prerequisites:</b> <span class="none">None</span></p>
    <table><tr><td class="num">1</td><td class="action">Click Forgot password</td><td class="expected">A login email arrives</td></tr></table></div>
</div>`;
}

const shown = () =>
  Array.from(document.querySelectorAll(".case"))
    .filter((c) => !c.classList.contains("hidden"))
    .map((c) => c.getAttribute("data-key"));

function search(field: string, text: string) {
  const sel = document.getElementById("tc-field") as HTMLSelectElement;
  sel.value = field;
  sel.dispatchEvent(new Event("change"));
  const input = document.getElementById("tc-search") as HTMLInputElement;
  input.value = text;
  input.dispatchEvent(new Event("input"));
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
  document.body.innerHTML = page();
  (window as unknown as { __tcmWireSearch: () => void }).__tcmWireSearch();
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
  expect(shown()).toEqual(["101", "d1"]);
  search("id", "#101");
  expect(shown()).toEqual(["101"]);
  search("id", "d1");
  expect(shown()).toEqual(["d1"]);
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
  const input = document.getElementById("tc-search") as HTMLInputElement;
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  expect(input.value).toBe("");
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
  (window as unknown as { __tcmWireSearch: () => void }).__tcmWireSearch();
  const input = document.getElementById("tc-search") as HTMLInputElement;
  input.value = "login";
  input.dispatchEvent(new Event("input"));
  expect(shown()).toEqual(["101", "d1"]);
});

// The swap() path that captures and restores the field's value only runs
// with the live-report globals (REPORT_REV / NOTE_PORT) wired in
// cases-page.js's outer IIFE at load time, the same guard casesPageMarks.test.ts
// works around by never exercising it. It was verified by reading the code
// (see the report for the exact lines) rather than by a unit here.

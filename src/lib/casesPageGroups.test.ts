/**
 * The review page grouped by area (View in browser while the Queue's Group
 * by area is on): the cases sit inside nested <details class='tc-group'>
 * sections. The page's script (src-tauri/web/cases-page.js) was written
 * for a flat list; these load it as-is into a page shaped like the grouped
 * one and check search, the bookmark and a live refresh still behave.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeAll, beforeEach, expect, test } from "vitest";

const SCOPE = "draft-groups";

const card = (key: string, title: string) =>
  `<div class='case' data-key='${key}'><h2><span class='seq'>1</span><span class='title'>${title}</span>` +
  `<button type='button' class='tc-mark' aria-pressed='false'></button></h2></div>`;

function page(): string {
  return `
<div class='page'>
  <div class='searchbar'>
    <div class='tc-searchbox'><select id='tc-field'><option value='all'>All fields</option><option value='title'>Title</option></select>
      <input id='tc-search' type='search'></div>
    <button id='tc-goto' type='button' class='hidden'>Go to bookmark</button>
    <span id='tc-count'></span>
  </div>
  <p id='tc-no-match' class='no-match hidden'>No test cases match your search.</p>
  <details class='tc-group' open data-area='events' data-level='0'><summary><span class='tc-group-name'>Events</span> <span class='tc-group-count'>(2)</span></summary><div class='tc-group-body'>
    ${card("d0", "Open the events list")}
    <details class='tc-group' open data-area='events / create' data-level='1'><summary><span class='tc-group-name'>Create</span> <span class='tc-group-count'>(1)</span></summary><div class='tc-group-body'>
      ${card("d1", "Create an event")}
    </div></details>
  </div></details>
  <details class='tc-group' open data-area='' data-level='0'><summary><span class='tc-group-name'>Ungrouped</span> <span class='tc-group-count'>(1)</span></summary><div class='tc-group-body'>
    ${card("d2", "Sign in")}
  </div></details>
</div>`;
}

type Hooks = {
  __tcmWireSearch: () => void;
  __tcmWireMarks: () => void;
  __tcmForgetMark: () => void;
  tcmPage: {
    openState: (root: ParentNode) => Record<string, boolean>;
    restoreOpen: (root: ParentNode, state: Record<string, boolean>) => void;
  };
};
const hooks = window as unknown as Hooks;

const group = (area: string) => document.querySelector<HTMLDetailsElement>(`details.tc-group[data-area='${area}']`)!;
const input = () => document.getElementById("tc-search") as HTMLInputElement;

function search(text: string) {
  input().value = text;
  input().dispatchEvent(new Event("input"));
}

beforeAll(() => {
  const here = dirname(fileURLToPath(import.meta.url));
  const src = readFileSync(resolve(here, "../../src-tauri/web/cases-page.js"), "utf8");
  document.body.setAttribute("data-scope", SCOPE);
  document.body.innerHTML = page();
  new Function(src)();
});

beforeEach(() => {
  localStorage.clear();
  hooks.__tcmForgetMark();
  document.body.innerHTML = page();
  hooks.__tcmWireSearch();
  hooks.__tcmWireMarks();
});

test("search finds cases inside nested sections and hides the sections it emptied", () => {
  search("create");
  expect(document.getElementById("tc-count")!.textContent).toBe("1 of 3 shown");
  expect(group("events").classList.contains("hidden")).toBe(false);
  expect(group("events / create").classList.contains("hidden")).toBe(false);
  expect(group("").classList.contains("hidden")).toBe(true);

  search("");
  expect(group("").classList.contains("hidden")).toBe(false);
  expect(document.getElementById("tc-count")!.textContent).toBe("3 test cases");
});

test("a folded section holding a match opens while searching", () => {
  group("events").removeAttribute("open");
  search("create an event");
  expect(group("events").open).toBe(true);
});

test("Go to bookmark opens the folded sections around the marked case", () => {
  document.querySelector<HTMLButtonElement>("[data-key='d1'] .tc-mark")!.click();
  group("events").removeAttribute("open");
  group("events / create").removeAttribute("open");
  document.getElementById("tc-goto")!.click();
  expect(group("events").open).toBe(true);
  expect(group("events / create").open).toBe(true);
});

test("a section the reader folded stays folded through a live refresh", () => {
  group("events / create").removeAttribute("open");
  const state = hooks.tcmPage.openState(document);
  document.body.innerHTML = page();
  hooks.tcmPage.restoreOpen(document, state);
  expect(group("events / create").open).toBe(false);
  expect(group("events").open).toBe(true);
  expect(group("").open).toBe(true);
});

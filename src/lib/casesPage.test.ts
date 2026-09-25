/**
 * The review page's live update (src-tauri/web/cases-page.js), loaded as-is
 * with the globals the app writes before it, a fake loopback listener, and
 * fake timers driving the 4 s poll.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(resolve(here, "../../src-tauri/web/cases-page.js"), "utf8");

type Page = {
  openState: (root: ParentNode) => Record<string, boolean>;
  restoreOpen: (root: ParentNode, state: Record<string, boolean>) => void;
  reload: () => void;
};
const tcmPage = () => (window as unknown as { tcmPage: Page }).tcmPage;

function page(title: string, note: string, seq = "1"): string {
  return `<div class="page">
    <div id="tc-stale" role="status"><span>changed</span><button type="button" id="tc-stale-go">Refresh</button></div>
    <div class="searchbar"><input id="tc-search" type="search">
      <button id="tc-findings" type="button" aria-pressed="false">Hide findings</button><span id="tc-count"></span></div>
    <div class="case"><h2><span class="seq">${seq}</span>${title}</h2>
      <div class="rev-wrap"><details class="rev" open><summary>Reviewer notes</summary><div class="rev-body">${note}</div></details></div>
      <div class="find-wrap"><details class="findings" open><summary>Findings (1)</summary><div>f</div></details></div>
    </div>
  </div>`;
}

let responses: { version: unknown; report: string | null };

beforeEach(() => {
  vi.useFakeTimers();
  localStorage.clear();
  document.body.className = "";
  document.body.innerHTML = page("Login", "old");
  Object.assign(globalThis, { REPORT_REV: 1, NOTE_PORT: 4711, NOTE_TOKEN: "t", NOTE_ORG: "", REPORT_KIND: "draft" });
  window.scrollTo = vi.fn() as unknown as typeof window.scrollTo;
  responses = { version: { revision: 2 }, report: `<!DOCTYPE html><html><body>${page("Login", "fresh", "2")}</body></html>` };
  vi.stubGlobal(
    "fetch",
    vi.fn((url: string) => {
      if (url.includes("/version")) return Promise.resolve({ ok: true, json: () => Promise.resolve(responses.version) });
      if (responses.report === null) return Promise.resolve({ ok: false, text: () => Promise.resolve("") });
      return Promise.resolve({ ok: true, text: () => Promise.resolve(responses.report) });
    }),
  );
  new Function(src)();
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  for (const k of ["REPORT_REV", "NOTE_PORT", "NOTE_TOKEN", "NOTE_ORG", "REPORT_KIND", "tcmNotes"]) {
    delete (globalThis as Record<string, unknown>)[k];
  }
});

async function poll() {
  await vi.advanceTimersByTimeAsync(4000);
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

test("after a swap, Refresh, the stale banner and the findings toggle all still work", async () => {
  await poll();
  expect(document.querySelector(".rev-body")!.textContent).toBe("fresh");

  const reload = vi.fn();
  tcmPage().reload = reload;
  (document.getElementById("tc-stale-go") as HTMLButtonElement).click();
  expect(reload).toHaveBeenCalled();

  document.getElementById("tc-findings")!.click();
  expect(document.body.classList.contains("findings-off")).toBe(true);
  expect(document.getElementById("tc-findings")!.textContent).toBe("Show findings");

  // The next change cannot be pulled: the banner in the CURRENT document shows.
  responses = { version: { revision: 3 }, report: null };
  await poll();
  expect(document.getElementById("tc-stale")!.classList.contains("show")).toBe(true);
});

test("findings hidden before a swap stay hidden, and the fresh button says so", async () => {
  document.getElementById("tc-findings")!.click();
  await poll();
  expect(document.querySelector(".rev-body")!.textContent).toBe("fresh");
  expect(document.body.classList.contains("findings-off")).toBe(true);
  expect(document.getElementById("tc-findings")!.textContent).toBe("Show findings");
});

test("a swap keeps each section open or closed as the reader left it, keyed by case title", async () => {
  document.querySelector(".case details.findings")!.removeAttribute("open");
  await poll(); // the fresh copy has both open, and the case moved to position 2
  expect(document.querySelector(".rev-body")!.textContent).toBe("fresh");
  expect(document.querySelector(".case details.rev")!.hasAttribute("open")).toBe(true);
  expect(document.querySelector(".case details.findings")!.hasAttribute("open")).toBe(false);
});

test("openState keys by the title without its position number, plus the section's class", () => {
  const state = tcmPage().openState(document);
  expect(Object.keys(state).sort()).toEqual(["Login\nfindings", "Login\nrev"]);
  tcmPage().restoreOpen(document, { "Login\nrev": false, "Login\nfindings": true });
  expect(document.querySelector(".case details.rev")!.hasAttribute("open")).toBe(false);
  expect(document.querySelector(".case details.findings")!.hasAttribute("open")).toBe(true);
});

// A reviewer can click out of a textarea before its 600 ms debounce fires,
// or while the save it armed is still in flight or queued behind another -
// the focused-textarea check alone misses all of that, and a swap during it
// would show the box's OLD text under text the reviewer already moved past.
test("the poll skips the swap while a comment box is busy (armed, in flight, or queued), and swaps once nothing is", async () => {
  (window as unknown as { tcmNotes: { busy: () => number } }).tcmNotes = { busy: () => 1 };
  await poll();
  expect(document.querySelector(".rev-body")!.textContent).toBe("old");

  (window as unknown as { tcmNotes: { busy: () => number } }).tcmNotes.busy = () => 0;
  await poll();
  expect(document.querySelector(".rev-body")!.textContent).toBe("fresh");
});

async function later(ms: number) {
  await vi.advanceTimersByTimeAsync(ms);
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

/// A closed app answers nothing, and every refused ask is an error line in
/// the browser's console. Each failure doubles the wait (to at most a
/// minute); the first answer brings the 4 s poll back.
test("a closed app is asked less and less often, and an answer brings the 4 s poll back", async () => {
  let up = false;
  const asked: string[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((url: string) => {
      asked.push(url);
      if (!up) return Promise.reject(new TypeError("Failed to fetch"));
      return Promise.resolve({ ok: true, json: () => Promise.resolve({ revision: 1 }) });
    }),
  );
  const versionAsks = () => asked.filter((u) => u.includes("/version")).length;

  await poll(); // 4 s: asked, refused - the next ask waits 8 s
  expect(versionAsks()).toBe(1);
  await poll(); // 8 s: not yet
  expect(versionAsks()).toBe(1);
  await poll(); // 12 s: asked, refused - the next waits 16 s
  expect(versionAsks()).toBe(2);
  await later(15_000); // 27 s: not yet
  expect(versionAsks()).toBe(2);

  up = true;
  await later(1_000); // 28 s: asked, answered
  expect(versionAsks()).toBe(3);
  await poll(); // 32 s: back to every 4 s
  expect(versionAsks()).toBe(4);
});

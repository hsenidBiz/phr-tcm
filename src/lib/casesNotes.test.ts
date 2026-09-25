/**
 * The review page's comment autosave (src-tauri/web/cases-notes.js): one
 * save in flight per box, the newest text next, only the newest reply shown.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";

type Report = (r: unknown, err: unknown) => void;
type Notes = {
  makeQueue: (send: (payload: string) => Promise<unknown>, report: Report) => (payload: string) => void;
  busy: () => number;
  timeoutMs: number;
  restoreUnsaved: (root?: ParentNode) => void;
};
let N: Notes;

beforeAll(() => {
  const here = dirname(fileURLToPath(import.meta.url));
  new Function(readFileSync(resolve(here, "../../src-tauri/web/cases-notes.js"), "utf8"))();
  N = (window as unknown as { tcmNotes: Notes }).tcmNotes;
});

const flush = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};

function harness() {
  const settle: Array<{ ok: (v: unknown) => void; fail: (e: unknown) => void }> = [];
  const sent: string[] = [];
  const reports: unknown[] = [];
  const send = vi.fn(
    (payload: string) =>
      new Promise((ok, fail) => {
        sent.push(payload);
        settle.push({ ok, fail });
      }),
  );
  const save = N.makeQueue(send, (r, err) => reports.push(err ? "error" : r));
  return { settle, sent, reports, save };
}

test("a save waits for the one in flight, and only the newest waiting text goes next", async () => {
  const h = harness();
  h.save("one");
  h.save("two");
  h.save("three");
  expect(h.sent).toEqual(["one"]);
  h.settle[0].ok({ ok: true, n: 1 });
  await flush();
  expect(h.sent).toEqual(["one", "three"]);
  expect(h.reports).toEqual([]); // the reply to "one" is stale - "three" is what the box holds
  h.settle[1].ok({ ok: true, n: 3 });
  await flush();
  expect(h.reports).toEqual([{ ok: true, n: 3 }]);
});

test("a failed newest save is reported, a failed older one is not", async () => {
  const h = harness();
  h.save("one");
  h.save("two");
  h.settle[0].fail(new Error("closed"));
  await flush();
  expect(h.reports).toEqual([]);
  h.settle[1].fail(new Error("closed"));
  await flush();
  expect(h.reports).toEqual(["error"]);
});

// busy() is what cases-page.js asks before swapping in a fresh copy: it must
// stay >0 for as long as a box has an edit not yet safely on disk - the
// 600ms debounce window, the save it fires in flight, and any later edit
// queued behind that save - or a swap mid-typing shows the box's OLD text
// under whatever the reviewer already typed next.
type Wire = { __tcmWireNotes: () => void };

beforeEach(() => {
  vi.useFakeTimers();
  document.body.innerHTML =
    '<textarea data-case="0" data-status="st"></textarea><div id="st"></div>' +
    `<script type="application/json" id="tc-data">${JSON.stringify({
      pbi: 1,
      cases: [{ path: "", id: null, title: "T", key: "t:t" }],
      files: [],
    })}</script>`;
  Object.assign(globalThis, { NOTE_PORT: 4711, NOTE_TOKEN: "t", NOTE_ORG: "" });
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
  for (const k of ["NOTE_PORT", "NOTE_TOKEN", "NOTE_ORG"]) {
    delete (globalThis as Record<string, unknown>)[k];
  }
});

test("busy() counts an armed debounce timer, an in-flight save, and a save queued behind it", async () => {
  const settle: Array<{ ok: (v: unknown) => void; fail: (e: unknown) => void }> = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(() => new Promise((ok, fail) => settle.push({ ok, fail }))),
  );
  (window as unknown as Wire).__tcmWireNotes();
  expect(N.busy()).toBe(0);

  const box = document.querySelector("textarea") as HTMLTextAreaElement;
  box.value = "hi";
  box.dispatchEvent(new Event("input"));
  expect(N.busy()).toBe(1); // debounce armed, nothing sent yet
  expect(settle.length).toBe(0);

  await vi.advanceTimersByTimeAsync(600);
  expect(settle.length).toBe(1); // the debounce fired: a save is now in flight
  expect(N.busy()).toBe(1);

  // A second edit lands while that save is still in flight.
  box.value = "hi there";
  box.dispatchEvent(new Event("input"));
  await vi.advanceTimersByTimeAsync(600);
  expect(settle.length).toBe(1); // queued behind the one in flight, not sent
  expect(N.busy()).toBe(1);

  settle[0].ok({ json: () => Promise.resolve({ ok: true }) });
  await flush();
  expect(settle.length).toBe(2); // the queued save goes out now
  expect(N.busy()).toBe(1);

  settle[1].ok({ json: () => Promise.resolve({ ok: true }) });
  await flush();
  expect(N.busy()).toBe(0);
});

/// A save the app never answers must still end, or the box stays "busy"
/// for good and the page's live update waits on it forever.
test("a save the app never answers ends after the limit, frees the box and says so", async () => {
  vi.stubGlobal("fetch", vi.fn(() => new Promise(() => {})));
  (window as unknown as Wire).__tcmWireNotes();
  const box = document.querySelector("textarea") as HTMLTextAreaElement;
  box.value = "hi";
  box.dispatchEvent(new Event("input"));
  await vi.advanceTimersByTimeAsync(600);
  expect(N.busy()).toBe(1);

  await vi.advanceTimersByTimeAsync(N.timeoutMs);
  await flush();
  expect(N.busy()).toBe(0);
  expect(document.getElementById("st")!.textContent).toBe("Not saved - the app did not answer");
});

/// Review Focus 4: slow is not dead. A reply inside the limit is saved.
test("a save the app answers slowly, inside the limit, is still saved", async () => {
  const gate: { answer?: (v: unknown) => void } = {};
  vi.stubGlobal(
    "fetch",
    vi.fn(
      () =>
        new Promise((ok) => {
          gate.answer = ok;
        }),
    ),
  );
  (window as unknown as Wire).__tcmWireNotes();
  const box = document.querySelector("textarea") as HTMLTextAreaElement;
  box.value = "hi";
  box.dispatchEvent(new Event("input"));
  await vi.advanceTimersByTimeAsync(600);
  await vi.advanceTimersByTimeAsync(N.timeoutMs - 1_000);
  gate.answer?.({ json: () => Promise.resolve({ ok: true }) });
  await flush();
  expect(document.getElementById("st")!.textContent).toBe("Saved ✓");

  // The limit passing afterwards changes nothing.
  await vi.advanceTimersByTimeAsync(2_000);
  await flush();
  expect(document.getElementById("st")!.textContent).toBe("Saved ✓");
  expect(N.busy()).toBe(0);
});

/// Review round 1, Important 1, refused path: `restoreUnsaved` is what
/// cases-page.js calls right after a live swap adopts fresh markup, so a
/// save the app refused survives it too - not just a timeout.
test("restoreUnsaved carries a refused save's text and status onto the matching fresh box", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(() => Promise.resolve({ json: () => Promise.resolve({ ok: false, error: "duplicate" }) })),
  );
  (window as unknown as Wire).__tcmWireNotes();
  const box = document.querySelector("textarea") as HTMLTextAreaElement;
  box.value = "unsaved edit";
  box.dispatchEvent(new Event("input"));
  await vi.advanceTimersByTimeAsync(600);
  await flush();
  expect(document.getElementById("st")!.textContent).toBe("Not saved — duplicate");

  // What a live swap does: fresh markup, the SAME case (same path + key in
  // the fresh #tc-data, even though it is still slot 0 here), the file's
  // OLDER text and no status - adopted in place of the old box.
  document.body.innerHTML =
    '<textarea data-case="0" data-status="st">older text from disk</textarea><div id="st"></div>' +
    `<script type="application/json" id="tc-data">${JSON.stringify({
      pbi: 1,
      cases: [{ path: "", id: null, title: "T", key: "t:t" }],
      files: [],
    })}</script>`;
  N.restoreUnsaved(document);
  expect((document.querySelector("textarea") as HTMLTextAreaElement).value).toBe("unsaved edit");
  expect(document.getElementById("st")!.textContent).toBe("Not saved — duplicate");
});

/// Review round 1 re-review, Important A: `data-case`/`data-file` are this
/// RENDER's slot, not a stable identity. Keying a carried failure by slot
/// alone would misfile it onto whatever case a swap moves into that slot.
test("restoreUnsaved keys a case box by path+key, not by slot - a case inserted ahead does not misfile the failed text", async () => {
  document.body.innerHTML =
    '<textarea data-case="0" data-status="st0"></textarea><div id="st0"></div>' +
    `<script type="application/json" id="tc-data">${JSON.stringify({
      pbi: 1,
      cases: [{ path: "f.json", id: null, title: "Login", key: "t:login#0" }],
      files: [],
    })}</script>`;
  vi.stubGlobal(
    "fetch",
    vi.fn(() => Promise.resolve({ json: () => Promise.resolve({ ok: false, error: "duplicate" }) })),
  );
  (window as unknown as Wire).__tcmWireNotes();
  const box0 = document.querySelector("textarea") as HTMLTextAreaElement;
  box0.value = "unsaved login note";
  box0.dispatchEvent(new Event("input"));
  await vi.advanceTimersByTimeAsync(600);
  await flush();
  expect(document.getElementById("st0")!.textContent).toBe("Not saved — duplicate");

  // A live swap inserts a NEW case ahead of it: "Login" is now slot 1.
  document.body.innerHTML =
    '<textarea data-case="0" data-status="stA"></textarea><div id="stA"></div>' +
    '<textarea data-case="1" data-status="stB"></textarea><div id="stB"></div>' +
    `<script type="application/json" id="tc-data">${JSON.stringify({
      pbi: 1,
      cases: [
        { path: "f.json", id: null, title: "New case", key: "t:new#0" },
        { path: "f.json", id: null, title: "Login", key: "t:login#0" },
      ],
      files: [],
    })}</script>`;
  N.restoreUnsaved(document);

  const boxes = Array.from(document.querySelectorAll("textarea")) as HTMLTextAreaElement[];
  // Slot 0 now belongs to the NEW case - it must not inherit the stale text.
  expect(boxes[0].value).toBe("");
  expect(document.getElementById("stA")!.textContent).toBe("");
  // Slot 1 is the ORIGINAL case, found by identity, not by slot.
  expect(boxes[1].value).toBe("unsaved login note");
  expect(document.getElementById("stB")!.textContent).toBe("Not saved — duplicate");
});

/// Same identity rule, the other named risk: once the failed case is gone
/// from the file, nothing should show its text - not even the box that
/// happens to inherit its old slot.
test("restoreUnsaved drops a failed comment once its case is removed - it is not shown under another box", async () => {
  document.body.innerHTML =
    '<textarea data-case="0" data-status="st0"></textarea><div id="st0"></div>' +
    `<script type="application/json" id="tc-data">${JSON.stringify({
      pbi: 1,
      cases: [{ path: "f.json", id: null, title: "Login", key: "t:login#0" }],
      files: [],
    })}</script>`;
  vi.stubGlobal(
    "fetch",
    vi.fn(() => Promise.resolve({ json: () => Promise.resolve({ ok: false, error: "duplicate" }) })),
  );
  (window as unknown as Wire).__tcmWireNotes();
  const box0 = document.querySelector("textarea") as HTMLTextAreaElement;
  box0.value = "unsaved login note";
  box0.dispatchEvent(new Event("input"));
  await vi.advanceTimersByTimeAsync(600);
  await flush();
  expect(document.getElementById("st0")!.textContent).toBe("Not saved — duplicate");

  // The failed case is removed from the file; a DIFFERENT case now sits at
  // the same slot 0.
  document.body.innerHTML =
    '<textarea data-case="0" data-status="stA"></textarea><div id="stA"></div>' +
    `<script type="application/json" id="tc-data">${JSON.stringify({
      pbi: 1,
      cases: [{ path: "g.json", id: null, title: "Other case", key: "t:other#0" }],
      files: [],
    })}</script>`;
  N.restoreUnsaved(document);

  const boxA = document.querySelector("textarea") as HTMLTextAreaElement;
  expect(boxA.value).toBe("");
  expect(document.getElementById("stA")!.textContent).toBe("");
});

/// A save that DID succeed must not leave a stale entry behind - the next
/// swap must show the fresh box exactly as the file has it.
test("restoreUnsaved does nothing once the box's save has succeeded", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(() => Promise.resolve({ json: () => Promise.resolve({ ok: true }) })),
  );
  (window as unknown as Wire).__tcmWireNotes();
  const box = document.querySelector("textarea") as HTMLTextAreaElement;
  box.value = "saved edit";
  box.dispatchEvent(new Event("input"));
  await vi.advanceTimersByTimeAsync(600);
  await flush();
  expect(document.getElementById("st")!.textContent).toBe("Saved ✓");

  document.body.innerHTML = '<textarea data-case="0" data-status="st">saved edit</textarea><div id="st"></div>';
  N.restoreUnsaved(document);
  expect((document.querySelector("textarea") as HTMLTextAreaElement).value).toBe("saved edit");
  expect(document.getElementById("st")!.textContent).toBe("");
});

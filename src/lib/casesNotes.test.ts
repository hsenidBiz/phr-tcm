/**
 * The review page's comment autosave (src-tauri/web/cases-notes.js): one
 * save in flight per box, the newest text next, only the newest reply shown.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeAll, expect, test, vi } from "vitest";

type Report = (r: unknown, err: unknown) => void;
type Notes = { makeQueue: (send: (payload: string) => Promise<unknown>, report: Report) => (payload: string) => void };
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

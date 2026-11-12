import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, expect, test } from "vitest";
import type { UploadJournal } from "../bindings";
import { submitFinished, submitPhaseSnapshot, submitStarted } from "./submitRun";
import { loadHold } from "./uploadHold";
import { finishJournal, olderPartOf, recoverJournal } from "./uploadJournal";

const J: UploadJournal = {
  organization: "acme",
  project: "Web",
  pbi_id: 42,
  since: "2026-11-11T10:00:00Z",
  titles: ["A", "B"],
  sigs: ["sa", "sb"],
  ids: [7],
};

beforeEach(() => localStorage.clear());
afterEach(() => {
  clearMocks();
  localStorage.clear();
  const p = submitPhaseSnapshot();
  if (p) submitFinished(p.run);
});

test("a found journal is saved as a hold and then cleared", async () => {
  const order: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "upload_journal_read") {
      order.push("read");
      return { kind: "found", journal: J };
    }
    if (cmd === "upload_journal_clear") {
      order.push(loadHold("acme", 42) ? "clear-after-hold" : "clear-before-hold");
      expect(args).toEqual({ organization: "acme", pbiId: 42 });
      return null;
    }
  });
  expect(await recoverJournal("acme", 42)).toBe("recovered");
  expect(order).toEqual(["read", "clear-after-hold"]);
  expect(loadHold("acme", 42)?.titles).toEqual(["A", "B"]);
});

test("an upload running in this window for this PBI is never read", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
  });
  const run = submitStarted("acme", 42, 1);
  expect(await recoverJournal("acme", 42)).toBe("running");
  expect(calls).not.toContain("upload_journal_read");
  expect(loadHold("acme", 42)).toBeNull();
  submitFinished(run as number);
});

test("an upload on another PBI does not block recovery here", async () => {
  mockIPC((cmd) => {
    if (cmd === "upload_journal_read") return { kind: "found", journal: J };
    if (cmd === "upload_journal_clear") return null;
  });
  const run = submitStarted("acme", 7, 1);
  expect(await recoverJournal("acme", 42)).toBe("recovered");
  submitFinished(run as number);
});

test("Rust saying running makes no hold", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "upload_journal_read") return { kind: "running" };
  });
  expect(await recoverJournal("acme", 42)).toBe("running");
  expect(calls).not.toContain("upload_journal_clear");
  expect(loadHold("acme", 42)).toBeNull();
});

test("nothing there, or no answer, is none", async () => {
  mockIPC((cmd) => {
    if (cmd === "upload_journal_read") return { kind: "none" };
  });
  expect(await recoverJournal("acme", 42)).toBe("none");
  clearMocks();
  mockIPC(() => undefined);
  expect(await recoverJournal("acme", 42)).toBe("none");
});

test("a journal beside the hold saved from the same upload does not double rows", async () => {
  localStorage.setItem(
    "tcm-v2-upload-hold:acme/42",
    JSON.stringify({ since: "2026-11-11T10:00:00Z", titles: ["A", "B"], ids: [7], sigs: ["sa", "sb"] }),
  );
  mockIPC((cmd) => {
    if (cmd === "upload_journal_read") return { kind: "found", journal: J };
    if (cmd === "upload_journal_clear") return null;
  });
  expect(await recoverJournal("acme", 42)).toBe("recovered");
  expect(loadHold("acme", 42)?.titles).toEqual(["A", "B"]);
});

const M: UploadJournal = {
  ...J,
  titles: ["O1", "O2", "O3", "N1", "N2"],
  sigs: ["o1", "o2", "o3", "n1", "n2"],
  ids: [7, 8],
};

const NEW = ["N1", "N2"];

test("olderPartOf keeps the leading rows and the older ids and since", () => {
  const o = olderPartOf(M, NEW);
  expect(o?.titles).toEqual(["O1", "O2", "O3"]);
  expect(o?.sigs).toEqual(["o1", "o2", "o3"]);
  expect(o?.ids).toEqual([7, 8]);
  expect(o?.since).toBe(M.since);
});

test("olderPartOf drops misaligned sigs, and is null when nothing is older", () => {
  expect(olderPartOf({ ...M, sigs: [] }, NEW)?.sigs).toEqual([]);
  expect(olderPartOf(M, M.titles)).toBeNull();
  expect(olderPartOf(M, [...M.titles, "X"])?.titles).toEqual(M.titles);
  expect(olderPartOf(M, [])?.titles).toEqual(M.titles);
});

function mockFinish(journal: UploadJournal | null, calls: string[], readError = false) {
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "upload_journal_read") {
      if (readError) throw new Error("boom");
      return journal ? { kind: "found", journal } : { kind: "none" };
    }
    if (cmd === "upload_journal_clear") {
      calls.push(loadHold("acme", 42) ? "hold-present" : "hold-absent");
      return null;
    }
  });
}

test("finishJournal turns the older rows into a hold with older ids plus created ids, then clears", async () => {
  const calls: string[] = [];
  mockFinish(M, calls);
  await finishJournal("acme", 42, NEW, [900]);
  const h = loadHold("acme", 42);
  expect(h?.titles).toEqual(["O1", "O2", "O3"]);
  expect(h?.ids?.slice().sort()).toEqual([7, 8, 900]);
  expect(calls).toContain("upload_journal_clear");
  expect(calls).toContain("hold-present");
});

test("finishJournal does not let this upload's preExisting hide the older upload's cases", async () => {
  localStorage.setItem(
    "tcm-v2-upload-hold:acme/42",
    JSON.stringify({ since: "2026-11-11T10:05:00Z", titles: ["N1"], ids: [7, 500, 901], sigs: ["n1"] }),
  );
  mockFinish(M, []);
  await finishJournal("acme", 42, NEW, [901]);
  const h = loadHold("acme", 42);
  expect(h?.titles).toEqual(expect.arrayContaining(["O1", "O2", "O3", "N1"]));
  expect(h?.ids).not.toContain(500);
  expect(h?.ids?.slice().sort()).toEqual([7, 8, 901]);
  expect(h?.since).toBe(M.since);
});

test("finishJournal with a journal of exactly this upload's size saves no hold and clears", async () => {
  const calls: string[] = [];
  mockFinish(M, calls);
  await finishJournal("acme", 42, M.titles, []);
  expect(loadHold("acme", 42)).toBeNull();
  expect(calls).toContain("upload_journal_clear");
});

test("finishJournal still clears when the read fails", async () => {
  const calls: string[] = [];
  mockFinish(M, calls, true);
  await finishJournal("acme", 42, NEW, []);
  expect(loadHold("acme", 42)).toBeNull();
  expect(calls).toContain("upload_journal_clear");
});

test("olderPartOf holds the whole journal when it does not end with this upload's titles", () => {
  const olderOnly: UploadJournal = { ...M, titles: ["O1", "O2", "O3"], sigs: ["o1", "o2", "o3"] };
  expect(olderPartOf(olderOnly, NEW)?.titles).toEqual(["O1", "O2", "O3"]);
  expect(olderPartOf(M, NEW)?.titles).toEqual(["O1", "O2", "O3"]);
});

test("finishJournal holds an older-only journal whole when this upload's write failed", async () => {
  mockFinish({ ...M, titles: ["O1", "O2", "O3"], sigs: [] }, []);
  await finishJournal("acme", 42, NEW, []);
  expect(loadHold("acme", 42)?.titles).toEqual(["O1", "O2", "O3"]);
});

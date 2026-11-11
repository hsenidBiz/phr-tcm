import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, expect, test } from "vitest";
import type { UploadJournal } from "../bindings";
import { submitFinished, submitPhaseSnapshot, submitStarted } from "./submitRun";
import { loadHold } from "./uploadHold";
import { recoverJournal } from "./uploadJournal";

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

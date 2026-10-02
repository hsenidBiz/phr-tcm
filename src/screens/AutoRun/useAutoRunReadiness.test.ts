// What the Auto Run screen needs before a run can go anywhere, worked out
// from answers the screen already has. `undefined` / `null` mean "not known
// yet" - never "missing" - so a slow read can never open the screen on
// Setup by itself.

import { renderHook } from "@testing-library/react";
import { expect, test } from "vitest";
import type { CaseScript } from "../../bindings";
import { useAutoRunReadiness } from "./useAutoRunReadiness";

type Input = Parameters<typeof useAutoRunReadiness>[0];

const READY: Input = {
  siteUrl: "https://qa.example.com/",
  signIn: "builtin",
  accountCount: 1,
  areaCount: 0,
  scripts: [],
  testFileNames: [],
};

const readiness = (over: Partial<Input> = {}) =>
  renderHook(() => useAutoRunReadiness({ ...READY, ...over })).result.current;

const uploading = (...files: string[]): CaseScript =>
  ({
    case_id: 1,
    title: "s",
    steps: [
      {
        step_number: 1,
        actions: files.map((file) => ({ kind: "upload", selector: { css: "#f" }, file })),
      },
    ],
  }) as unknown as CaseScript;

test("a site address, a sign-in and an account are ready", () => {
  expect(readiness()).toEqual({ loaded: true, essentialMissing: false, missingTestFiles: [] });
  expect(readiness({ signIn: "saved" }).essentialMissing).toBe(false);
});

test("no site address, no sign-in or no accounts is essential and missing", () => {
  expect(readiness({ siteUrl: "" }).essentialMissing).toBe(true);
  expect(readiness({ siteUrl: null }).essentialMissing).toBe(true);
  expect(readiness({ signIn: "none" }).essentialMissing).toBe(true);
  expect(readiness({ accountCount: 0 }).essentialMissing).toBe(true);
});

test("an answer still to come is not loaded, and is never counted as missing", () => {
  const pending = readiness({ siteUrl: undefined, signIn: null, accountCount: null });
  expect(pending).toEqual({ loaded: false, essentialMissing: false, missingTestFiles: [] });
  // One known gap is enough to say so, while the rest is still loading.
  expect(readiness({ siteUrl: "", accountCount: null })).toEqual(
    expect.objectContaining({ loaded: false, essentialMissing: true }),
  );
});

test("areas and test files never hold up the decision", () => {
  expect(readiness({ areaCount: null, testFileNames: null }).loaded).toBe(true);
});

test("a test file a script uploads but the folder lacks is missing, once, in name order", () => {
  const r = readiness({
    scripts: [uploading("cv.txt", "b.pdf"), null, undefined, uploading("b.pdf", "a.png")],
    testFileNames: ["CV.TXT"],
  });
  // Windows file names: the folder's cv file is the script's, whatever the case.
  expect(r.missingTestFiles).toEqual(["a.png", "b.pdf"]);
  // Until the folder has been read nothing can be called missing.
  expect(readiness({ scripts: [uploading("x.pdf")], testFileNames: null }).missingTestFiles).toEqual([]);
});

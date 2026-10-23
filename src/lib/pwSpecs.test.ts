import { expect, test } from "vitest";
import { SUITES_MISSING, specsPill, type SpecsView } from "./pwSpecs";

const view = (over: Partial<SpecsView>): SpecsView => ({ walk: null, processed: null, processing: false, failed: null, ...over });
const found = (count: number, fingerprint = "a") => ({ count, fingerprint });

test("nothing is said before the first look answers", () => {
  expect(specsPill(view({}))).toBeNull();
});

test("found specs that were read on this fingerprint are green", () => {
  expect(specsPill(view({ walk: found(12), processed: "a" }))).toMatchObject({ tone: "success", label: "12 specs ready" });
  expect(specsPill(view({ walk: found(1), processed: "a" }))?.label).toBe("1 spec ready");
});

test("a fingerprint that moved on, or a read going now, is amber", () => {
  expect(specsPill(view({ walk: found(3, "b"), processed: "a" }))).toMatchObject({ tone: "warning", label: "Updating specs…" });
  expect(specsPill(view({ walk: found(3, "a"), processed: "a", processing: true }))).toMatchObject({ tone: "warning" });
});

test("no specs, or no suites folder, is red: No specs found", () => {
  expect(specsPill(view({ walk: found(0) }))).toMatchObject({ tone: "danger", label: "No specs found" });
  expect(specsPill(view({ walk: { error: SUITES_MISSING } }))).toMatchObject({ tone: "danger", label: "No specs found", why: SUITES_MISSING });
});

test("a walk or a read that failed is red: Specs could not be read, and says why", () => {
  expect(specsPill(view({ walk: { error: "Locked." } }))).toMatchObject({ tone: "danger", label: "Specs could not be read", why: "Locked." });
  const failed = specsPill(view({ walk: found(2, "b"), processed: "a", failed: { fingerprint: "b", error: "It broke." } }));
  expect(failed).toMatchObject({ tone: "danger", label: "Specs could not be read" });
  expect(failed?.why).toContain("It broke.");
  // A failure at an older fingerprint is history: the new one is being read.
  expect(specsPill(view({ walk: found(2, "c"), processed: "a", failed: { fingerprint: "b", error: "x" } }))?.tone).toBe("warning");
});

test("the missing-suites sentence is the one Rust says", () => {
  // tests/suite/pw_repo_specs_status.rs pins the same sentence.
  expect(SUITES_MISSING).toBe("The clone has no suites folder.");
});

import { expect, test } from "vitest";
import { SUITES_MISSING, ruleFileName, specsPill, type SpecsView } from "./pwSpecs";

const view = (over: Partial<SpecsView>): SpecsView => ({ walk: null, processed: null, processing: false, failed: null, ...over });
const AGENTS = [
  "playwright-test-planner.md",
  "playwright-test-generator.md",
  "playwright-test-healer.md",
  "spec-editor.md",
  "test-refactorer.md",
  "doc-drift-auditor.md",
].map((a) => `.claude/agents/${a}`);
const RULES = [
  ...AGENTS,
  ".claude/commands/gen-test.md",
  ".claude/skills/edit-spec/SKILL.md",
  ".claude/skills/manage-test-users/SKILL.md",
  ".claude/skills/test-data/SKILL.md",
  "CLAUDE.md",
];
const found = (count: number, fingerprint = "a", missing: string[] = []) => ({
  count,
  fingerprint,
  pipeline: { found: RULES.filter((r) => !missing.includes(r)), missing },
});

test("nothing is said before the first look answers", () => {
  expect(specsPill(view({}))).toBeNull();
});

// The pill counts the rule files the script pipeline follows, never the
// clone's test specs: 86 specs and 11 rule files is "11 pipeline files".
test("every pipeline rule file in the clone is green, counted by kind", () => {
  const pill = specsPill(view({ walk: found(86), processed: "a" }));
  expect(pill).toMatchObject({ tone: "success", label: "11 pipeline files ready" });
  expect(pill?.why).toBe(
    "11 files the script pipeline follows were found in the clone: 6 agents, gen-test, 3 skills and CLAUDE.md. They are checked every minute and when this window comes back into focus.",
  );
  expect(pill?.why).not.toMatch(/86|spec/);
  // A clone with no test specs at all is no problem for the pipeline.
  expect(specsPill(view({ walk: found(0), processed: "a" }))?.tone).toBe("success");
});

test("a rule file the clone lacks is amber and named, with the shipped copy standing in", () => {
  const one = specsPill(view({ walk: found(86, "a", [".claude/agents/test-refactorer.md"]), processed: "a" }));
  expect(one).toMatchObject({ tone: "warning", label: "1 pipeline file missing" });
  expect(one?.why).toMatch(/^test-refactorer\.md is not in the clone, so the copy shipped with this version of Test Case Manager is used instead\./);
  const two = specsPill(
    view({ walk: found(86, "a", ["CLAUDE.md", ".claude/skills/edit-spec/SKILL.md"]), processed: "a" }),
  );
  expect(two).toMatchObject({ tone: "warning", label: "2 pipeline files missing" });
  expect(two?.why).toMatch(/^CLAUDE\.md and edit-spec\/SKILL\.md are not in the clone/);
});

test("a rule file is named as a person knows it", () => {
  expect(ruleFileName(".claude/agents/playwright-test-healer.md")).toBe("playwright-test-healer.md");
  expect(ruleFileName(".claude/skills/test-data/SKILL.md")).toBe("test-data/SKILL.md");
  expect(ruleFileName(".claude/commands/gen-test.md")).toBe("gen-test.md");
  expect(ruleFileName("CLAUDE.md")).toBe("CLAUDE.md");
});

test("a fingerprint that moved on, or a read going now, is amber", () => {
  expect(specsPill(view({ walk: found(3, "b"), processed: "a" }))).toMatchObject({ tone: "warning", label: "Updating specs…" });
  expect(specsPill(view({ walk: found(3, "a"), processed: "a", processing: true }))).toMatchObject({ tone: "warning" });
});

test("no suites folder is red: No specs found", () => {
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

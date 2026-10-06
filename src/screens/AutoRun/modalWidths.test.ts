import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { MODAL_LARGE } from "./modalWidths";

// The dialogs that show data share one width, so a new one cannot drift to
// its own. Read from source: jsdom does no layout, so a class is all a test
// can see.
const LARGE = [
  "ScriptEditor",
  "RunPane",
  "ReplayPane",
  "RunReview",
  "AccountsDialog",
  "AreasDialog",
  "RecipeEditor",
  "RecordSignInDialog",
  "ExecutionOrderDialog",
  "TestFilesDialog",
];

test.each(LARGE)("%s uses the shared large modal class", (name) => {
  const src = readFileSync(`src/screens/AutoRun/${name}.tsx`, "utf8");
  expect(src).toContain("${MODAL_LARGE}");
  expect(src).toContain('from "./modalWidths"');
});

test("the large class is wide, uncapped by max-w, and 90vh tall", () => {
  expect(MODAL_LARGE).toContain("w-[min(94vw,1400px)]");
  expect(MODAL_LARGE).toContain("max-w-none");
  expect(MODAL_LARGE).toContain("max-h-[90vh]");
});

test.each(["SiteAddressDialog", "SaveWordsDialog"])("%s is the short form", (name) => {
  expect(readFileSync(`src/screens/AutoRun/${name}.tsx`, "utf8")).toContain("max-w-2xl");
});

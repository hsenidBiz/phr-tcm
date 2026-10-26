/**
 * Every page search is the shared SearchInput (40px, magnifier, clear
 * button). jsdom does no layout, so this reads the sources: each box below
 * must be a `<SearchInput` carrying its aria-label, never a bare `<Input`.
 * The inner "Search..." boxes of the combobox and multiselect live inside a
 * dropdown and stay as they are.
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";

const SRC = dirname(fileURLToPath(import.meta.url));

const SEARCHES: Array<[file: string, label: string]> = [
  ["components/PbiPicker.tsx", "Find PBI"],
  ["screens/AutoRun/CaseSearch.tsx", "Search test cases"],
  ["screens/AutoRun/AccountsDialog.tsx", "Search proposed logins"],
  ["screens/ExistingCases/index.tsx", "Search test cases"],
  ["screens/ViewCases/index.tsx", "Search test cases"],
  ["screens/RunPanel/index.tsx", "Filter points"],
  ["screens/Suites.tsx", "Search suites"],
  ["screens/WorkBoard.tsx", "Filter items"],
  ["components/RelinkDialog.tsx", "Search for the destination PBI"],
  ["components/PipelineDialog.tsx", "Search stages and steps"],
];

test.each(SEARCHES)("%s: the %s box is a SearchInput", (file, label) => {
  const text = readFileSync(join(SRC, file), "utf-8");
  const at = text.indexOf(`aria-label="${label}"`);
  expect(at, `${label} not found in ${file}`).toBeGreaterThan(-1);
  const open = text.lastIndexOf("<", at);
  expect(text.slice(open, open + 12)).toMatch(/^<SearchInput\b/);
});

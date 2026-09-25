// A large group (a hundred-odd cases) unfolded, slowed down partway, then
// showed the rest at once. Its rows are `.cv-row` (content-visibility:
// auto), and a growing fold clips its content: every row below the moving
// clip edge was skipped, rendered a band at a time as the edge uncovered
// it, and whatever was left rendered in one go when the clip came off.
// While a fold grows, the rows it can reach render like any other content -
// only those (collapse.tsx marks them), so the hundreds below the window in a
// big group stay skipped.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";

const css = readFileSync(resolve(__dirname, "index.css"), "utf8");

/** The declarations of the first rule whose selector is exactly `selector`. */
function rule(source: string, selector: string): string {
  const at = source.search(new RegExp(`(^|\\n|,)\\s*${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*\\{`));
  expect(at, `found ${selector} { ... }`).toBeGreaterThan(-1);
  const open = source.indexOf("{", at);
  return source.slice(open + 1, source.indexOf("}", open));
}

test("the rows a growing fold marks are rendered, not skipped", () => {
  expect(rule(css, ".t-collapse.is-entering .cv-row[data-unfolding]")).toMatch(/content-visibility:\s*visible\s*;/);
});

test("no rule renders every row of a growing fold", () => {
  expect(css).not.toMatch(/\.t-collapse\.is-entering\s+\.cv-row\s*[,{]/);
});

test("long-list rows are still skipped off screen once settled", () => {
  expect(rule(css, ".cv-row")).toMatch(/content-visibility:\s*auto\s*;/);
});

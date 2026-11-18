// A group header row sits as far below the group above it as above its cards.
// jsdom does no layout, so the spacing is one shared set of classes
// (src/components/ui/group-header.ts) and this checks that every grouped
// screen takes its header row from it instead of its own padding.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "vitest";
import { GROUP_BODY, GROUP_COLUMN, GROUP_HEADER_ROW } from "../src/components/ui/group-header";

const SCREENS = [
  "screens/AutoRun/index.tsx",
  "screens/ViewCases/index.tsx",
  "screens/ExistingCases/index.tsx",
  "screens/RunPanel/index.tsx",
  "screens/ApiTemplates/index.tsx",
  "components/queue/QueueAreaGroup.tsx",
];

test("the shared row has 4px under it and nothing above; the column is 8px and the body 4px", () => {
  expect(GROUP_HEADER_ROW).toContain("pb-1");
  expect(GROUP_HEADER_ROW).not.toMatch(/\bpt-\d/);
  expect(GROUP_COLUMN).toBe("space-y-2");
  expect(GROUP_BODY).toBe("space-y-1");
});

test.each(SCREENS)("%s takes its group header row from the shared constant", (p) => {
  const src = readFileSync(join(process.cwd(), "src", p), "utf8");
  expect(src).toMatch(/import \{[^}]*GROUP_HEADER_ROW[^}]*\} from "[./]+(components\/)?ui\/group-header"|from "\.\.\/ui\/group-header"/);
  expect(src).toContain("GROUP_HEADER_ROW}");
  expect(src).not.toMatch(/gap-3 (px-2 )?pb-1/);
});

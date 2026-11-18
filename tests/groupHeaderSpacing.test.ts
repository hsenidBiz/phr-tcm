// A group header row sits as far below the group above it as above the cards
// under it. jsdom does no layout, so this reads the classes that decide the
// gap: each screen's parent gap, the group wrapper's gap, and the header
// row's own padding. Above = parent gap + header top padding; below = header
// bottom padding + wrapper gap. They must be equal.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "vitest";

const read = (p: string) => readFileSync(join(process.cwd(), "src", p), "utf8");

const HEADER = /className="flex w-full items-center gap-3( px-2)? (pb-1(?: pt-\d)?)"/;

function header(p: string): string {
  const m = HEADER.exec(read(p));
  expect(m, `${p} has no group header row`).not.toBeNull();
  return m![2];
}

test("Auto Run, View, Update and API templates headers drop the top padding (parent gap 8 = bottom 8)", () => {
  for (const p of ["screens/AutoRun/index.tsx", "screens/ViewCases/index.tsx", "screens/ExistingCases/index.tsx", "screens/ApiTemplates/index.tsx"]) {
    expect(header(p), p).toBe("pb-1");
  }
});

test("the queue's area headers and Run Tests' group headers use pt-1 (parent gap 4 or none)", () => {
  expect(header("components/queue/QueueAreaGroup.tsx")).toBe("pb-1 pt-1");
  expect(header("screens/RunPanel/index.tsx")).toBe("pb-1 pt-1");
});

test("the API templates groups sit in an 8px column so the header's gap above is 8", () => {
  expect(read("screens/ApiTemplates/index.tsx")).toMatch(/<div className="space-y-2">\s*\{groups\.map/);
});

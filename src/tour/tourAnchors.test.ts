/**
 * Every anchor the tour rings has to exist in the app. A screen can be
 * rewritten freely - it just has to keep its data-tour attribute, or the
 * tour silently loses a stop.
 */
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import { TOUR_ANCHORS } from "./tourScript";

const SRC = join(dirname(fileURLToPath(import.meta.url)), "..");

function sources(): string {
  let text = "";
  const walk = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name);
      if (e.isDirectory()) {
        if (e.name === "dev") continue; // dev tools never ship
        walk(p);
      } else if (/\.tsx$/.test(e.name) && !/\.test\.tsx$/.test(e.name)) {
        text += readFileSync(p, "utf8");
      }
    }
  };
  walk(SRC);
  return text;
}

test("every anchor the script may ring exists in a screen", () => {
  const text = sources();
  for (const name of TOUR_ANCHORS) {
    // Sidebar rows build theirs as `data-tour={`nav-${id}`}`, so accept
    // the literal or the sidebar's template.
    const present =
      text.includes(`data-tour="${name}"`) ||
      (name.startsWith("nav-") && text.includes("data-tour={`nav-${id}`}"));
    expect(present, `no data-tour="${name}" anywhere in src/`).toBe(true);
  }
});

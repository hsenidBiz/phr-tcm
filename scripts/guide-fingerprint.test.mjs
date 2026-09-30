import { afterEach, describe, expect, test } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fingerprint } from "./guide-fingerprint.mjs";

const made = [];
function site(files) {
  const root = mkdtempSync(join(tmpdir(), "guide-fp-"));
  made.push(root);
  for (const [rel, bytes] of Object.entries(files)) {
    const full = join(root, ...rel.split("/"));
    mkdirSync(join(full, ".."), { recursive: true });
    writeFileSync(full, bytes);
  }
  return root;
}
afterEach(() => {
  while (made.length) rmSync(made.pop(), { recursive: true, force: true });
});

describe("guide fingerprint", () => {
  // The SAME literal src-tauri/tests/suite/guide.rs pins for the same
  // fixture. If the two implementations ever disagree, every install would
  // see "Update Guide" forever - change both or neither.
  test("the_fingerprint_matches_the_apps", () => {
    const root = site({
      "index.html": "<html>",
      "img/a.jpg": Buffer.from([1, 2, 3]),
    });
    expect(fingerprint(root)).toBe(
      "9a2b1db7d7bd429fc1a0a310563276e3e2310674d59155fcec5f89b5c5096b74",
    );
  });

  test("the_fingerprint_ignores_creation_order", () => {
    const a = site({ "b.txt": "b", "a.txt": "a", "z/y.txt": "y" });
    const b = site({ "z/y.txt": "y", "a.txt": "a", "b.txt": "b" });
    expect(fingerprint(a)).toBe(fingerprint(b));
  });

  test("the_fingerprint_changes_with_content_and_with_path", () => {
    const base = fingerprint(site({ "a.txt": "a" }));
    expect(fingerprint(site({ "a.txt": "b" }))).not.toBe(base);
    expect(fingerprint(site({ "c.txt": "a" }))).not.toBe(base);
  });

  test("paths_sort_by_utf8_bytes_not_utf16_units", () => {
    // U+FF5E (3 UTF-8 bytes EF BD 9E) vs U+1F600 (4 bytes F0 9F 98 80):
    // bytes put U+FF5E first; UTF-16 code units put U+1F600 (D83D) first.
    const root = site({ "\uFF5E.txt": "x", "\u{1F600}.txt": "y" });
    const again = site({ "\u{1F600}.txt": "y", "\uFF5E.txt": "x" });
    expect(fingerprint(root)).toBe(fingerprint(again));
  });
});

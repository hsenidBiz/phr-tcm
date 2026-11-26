// The flow map's failure line is text-muted on the stage's surface-2 box:
// small text, so 4.5:1 in the light and the default dark theme.
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";

const css = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "../../src/index.css"), "utf8");

function token(block: string, name: string): string {
  const at = css.indexOf(block);
  const m = new RegExp(`--color-${name}: *(#[0-9a-fA-F]{6})`).exec(css.slice(at));
  if (!m) throw new Error(`${name} in ${block}`);
  return m[1];
}
function lum(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const [r, g, b] = c.map((x) => (x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
const ratio = (a: string, b: string) => {
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

test.each([
  ["light", "color-scheme: light;"],
  ["dark", ".dark {"],
])("%s: the failure line (text-muted) reads on the stage's surface-2 at 4.5:1", (_name, block) => {
  const r = ratio(token(block, "muted"), token(block, "surface-2"));
  expect(r).toBeGreaterThanOrEqual(4.5);
});

import { expect, test } from "vitest";
import { GLOSSARY, type TermName } from "../../src/lib/glossary";

const TERMS: TermName[] = [
  "recipe",
  "template",
  "fixture",
  "flow",
  "area",
  "component",
  "quirk",
  "divergence",
  "ruling",
];

test("every term has a plain one- or two-sentence definition", () => {
  expect(Object.keys(GLOSSARY).sort()).toEqual([...TERMS].sort());
  for (const name of TERMS) {
    const text = GLOSSARY[name];
    expect(text.length, name).toBeGreaterThan(30);
    const sentences = text.split(/(?<=\.)\s+/).filter(Boolean);
    expect(sentences.length, name).toBeLessThanOrEqual(2);
    expect(text.endsWith("."), name).toBe(true);
  }
});

test("a definition names no URL or file path", () => {
  for (const text of Object.values(GLOSSARY)) {
    expect(text).not.toMatch(/https?:|[A-Za-z]:\|\.json\b/);
  }
});

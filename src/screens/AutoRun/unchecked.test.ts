import { describe, expect, it } from "vitest";
import { uncheckedText } from "./unchecked";

describe("uncheckedText", () => {
  it("gives a sentence as it stands, trimmed", () => {
    expect(uncheckedText("  a  ")).toBe("a");
  });

  it("gives the entries as item - why, one after another", () => {
    expect(
      uncheckedText([
        { item: "no lock icon", why: "why one" },
        { item: "the click", why: "why two", stand_in: "expect_hidden" },
      ]),
    ).toBe("no lock icon - why one; the click - why two");
  });

  it("gives null for none, an empty list or a blank sentence", () => {
    expect(uncheckedText([])).toBeNull();
    expect(uncheckedText("")).toBeNull();
    expect(uncheckedText("   ")).toBeNull();
    expect(uncheckedText(null)).toBeNull();
    expect(uncheckedText(undefined)).toBeNull();
  });
});

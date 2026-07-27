import { expect, test } from "vitest";
import { applyMd } from "./markdownEdit";

/** Applies an action to `before`, where the selection is written as |...|
 * (or a bare | for a collapsed caret), and returns the result with the
 * new selection marked the same way. Keeps the cases readable. */
function run(before: string, action: Parameters<typeof applyMd>[3]): string {
  const first = before.indexOf("|");
  const rest = before.slice(first + 1).indexOf("|");
  const start = first;
  const end = rest === -1 ? first : first + rest;
  const value = before.replace(/\|/g, "");
  const r = applyMd(value, start, end, action);
  return r.value.slice(0, r.start) + "|" + r.value.slice(r.start, r.end) + "|" + r.value.slice(r.end);
}

test("wrapping keeps the text selected inside the markers", () => {
  expect(run("say |hello| there", "bold")).toBe("say **|hello|** there");
  expect(run("say |hello| there", "italic")).toBe("say _|hello|_ there");
  expect(run("say |hello| there", "code")).toBe("say `|hello|` there");
});

test("wrapping toggles off when the selection is already wrapped", () => {
  expect(run("say |**hello**| there", "bold")).toBe("say |hello| there");
});

test("wrapping toggles off when the markers sit just outside the selection", () => {
  // What you get after bolding, then clicking Bold again without moving.
  expect(run("say **|hello|** there", "bold")).toBe("say |hello| there");
});

test("an empty selection leaves the caret between the markers", () => {
  expect(run("type |here", "bold")).toBe("type **||**here");
});

test("a link puts the caret on the url, with the selection as the text", () => {
  expect(run("see |the docs| now", "link")).toBe("see [the docs](|url|) now");
});

test("a link with nothing selected still gives a usable skeleton", () => {
  expect(run("see |", "link")).toBe("see [text](|url|)");
});

test("list prefixes apply to every line the selection touches", () => {
  expect(run("a|lpha\nbeta\ngam|ma", "bullet")).toBe("|- alpha\n- beta\n- gamma|");
});

test("numbered lists actually count", () => {
  expect(run("|alpha\nbeta\ngamma|", "number")).toBe("|1. alpha\n2. beta\n3. gamma|");
});

test("a prefix toggles off when every line already has it", () => {
  expect(run("|- alpha\n- beta|", "bullet")).toBe("|alpha\nbeta|");
  expect(run("|1. alpha\n2. beta|", "number")).toBe("|alpha\nbeta|");
});

test("a partly-prefixed selection gets prefixed rather than stripped", () => {
  expect(run("|- alpha\nbeta|", "bullet")).toBe("|- - alpha\n- beta|");
});

test("heading and quote prefix the whole line from a bare caret", () => {
  expect(run("al|pha", "heading")).toBe("|## alpha|");
  expect(run("al|pha", "quote")).toBe("|> alpha|");
});

test("a prefix on a middle line does not disturb its neighbours", () => {
  expect(run("alpha\nbe|ta\ngamma", "quote")).toBe("alpha\n|> beta|\ngamma");
});

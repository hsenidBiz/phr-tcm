import { afterEach, expect, test } from "vitest";
import { SEQUENCE, SEQUENCE_LENGTH, SHAKE_FROM, isEditableTarget, next } from "./extrasSequence";

const run = (keys: readonly string[], from = 0) => keys.reduce((p, k) => next(p, k), from);

afterEach(() => {
  document.body.innerHTML = "";
});

test("the sequence is the eleven inputs the spec names, shaking from the fifth", () => {
  expect(SEQUENCE).toEqual([
    "ArrowUp", "ArrowUp", "ArrowDown", "ArrowDown",
    "ArrowLeft", "ArrowRight", "ArrowLeft", "ArrowRight",
    "b", "a", "Enter",
  ]);
  expect(SEQUENCE_LENGTH).toBe(11);
  expect(SHAKE_FROM).toBe(5);
});

test("each correct input moves one step, and the last one completes", () => {
  let p = 0;
  SEQUENCE.forEach((key, i) => {
    p = next(p, key);
    expect(p).toBe(i + 1);
  });
  expect(p).toBe(SEQUENCE_LENGTH);
});

test("letters count in either case", () => {
  expect(run([...SEQUENCE.slice(0, 8), "B", "A", "Enter"])).toBe(SEQUENCE_LENGTH);
  expect(run([...SEQUENCE.slice(0, 8), "b", "A", "Enter"])).toBe(SEQUENCE_LENGTH);
});

test("a modifier pressed on its own is not an input: Shift+B does not reset", () => {
  for (const mod of ["Shift", "Control", "Alt", "AltGraph", "Meta", "CapsLock"]) {
    for (let p = 0; p < SEQUENCE_LENGTH; p++) expect(next(p, mod), `${mod} at ${p}`).toBe(p);
  }
  expect(run([...SEQUENCE.slice(0, 8), "Shift", "B", "Shift", "A", "Enter"])).toBe(SEQUENCE_LENGTH);
});

test("a wrong key resets progress", () => {
  expect(next(7, "x")).toBe(0);
  expect(next(10, "b")).toBe(0);
  expect(next(3, "ArrowRight")).toBe(0);
  expect(next(0, "x")).toBe(0);
  expect(next(0, "Enter")).toBe(0);
  // Enter anywhere but last is simply wrong.
  expect(next(4, "Enter")).toBe(0);
});

test("a wrong ArrowUp counts as the first input of a new attempt", () => {
  for (const p of [3, 4, 5, 6, 7, 8, 9, 10]) expect(next(p, "ArrowUp"), `at ${p}`).toBe(1);
  // ...and that new attempt completes with the ten inputs after the first.
  expect(run(SEQUENCE.slice(1), next(7, "ArrowUp"))).toBe(SEQUENCE_LENGTH);
});

test("ArrowUp ArrowUp ArrowUp keeps progress at two", () => {
  expect(run(["ArrowUp", "ArrowUp", "ArrowUp"])).toBe(2);
  expect(run(["ArrowUp", "ArrowUp", "ArrowUp", "ArrowUp"])).toBe(2);
  expect(run(["ArrowUp", "ArrowUp", "ArrowUp", ...SEQUENCE.slice(2)])).toBe(SEQUENCE_LENGTH);
});

test("a progress outside the sequence starts again from nothing", () => {
  for (const p of [SEQUENCE_LENGTH, -1, 99, Number.NaN]) {
    expect(next(p, "ArrowUp"), `from ${p}`).toBe(1);
    expect(next(p, "x"), `from ${p}`).toBe(0);
  }
});

test("typing targets are editable: inputs, text areas, selects and contenteditable", () => {
  document.body.innerHTML = `
    <input id="i" /><input id="cb" type="checkbox" /><textarea id="t"></textarea>
    <select id="s"><option>a</option></select>
    <div id="ce" contenteditable="true"><span id="inside">x</span></div>
    <div id="ce2" contenteditable=""></div>
    <div id="off" contenteditable="false"></div>`;
  for (const id of ["i", "cb", "t", "s", "ce", "inside", "ce2"]) {
    expect(isEditableTarget(document.getElementById(id)), id).toBe(true);
  }
  expect(isEditableTarget(document.getElementById("off"))).toBe(false);
});

test("inside an open listbox, menu or a select's trigger counts as editable too", () => {
  document.body.innerHTML = `
    <div role="listbox"><div id="opt" role="option">a</div></div>
    <div role="menu"><button id="item" role="menuitem">b</button></div>
    <button id="trigger" role="combobox">c</button>`;
  for (const id of ["opt", "item", "trigger"]) {
    expect(isEditableTarget(document.getElementById(id)), id).toBe(true);
  }
});

test("everything else is not: a plain button, the body, the window, nothing", () => {
  document.body.innerHTML = `<button id="b">Go</button>`;
  expect(isEditableTarget(document.getElementById("b"))).toBe(false);
  expect(isEditableTarget(document.body)).toBe(false);
  expect(isEditableTarget(window)).toBe(false);
  expect(isEditableTarget(document)).toBe(false);
  expect(isEditableTarget(null)).toBe(false);
});

import { afterEach, expect, test } from "vitest";
import { cssLengthPx } from "./cssLength";

const root = document.documentElement;
afterEach(() => {
  root.style.removeProperty("--test-len");
  root.style.fontSize = "";
});

test("rem lengths convert by the root font size", () => {
  root.style.fontSize = "16px";
  root.style.setProperty("--test-len", "0.375rem");
  expect(cssLengthPx("--test-len", 99)).toBe(6);
  root.style.fontSize = "20px";
  expect(cssLengthPx("--test-len", 99)).toBe(7.5);
});

test("px lengths pass through", () => {
  root.style.setProperty("--test-len", "12px");
  expect(cssLengthPx("--test-len", 99)).toBe(12);
});

test("an unset or unsupported value gives the fallback", () => {
  expect(cssLengthPx("--test-len", 6)).toBe(6);
  root.style.setProperty("--test-len", "calc(1rem + 2px)");
  expect(cssLengthPx("--test-len", 6)).toBe(6);
});

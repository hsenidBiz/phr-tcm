import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, render, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import HalloweenDecor from "./HalloweenDecor";

const root = document.documentElement;
const css = readFileSync(resolve(__dirname, "../index.css"), "utf8");

afterEach(() => root.removeAttribute("data-theme"));

test("T2.a: it appears only on the Halloween theme and follows the theme live", async () => {
  const { container, findByText } = render(<HalloweenDecor />);
  expect(container.innerHTML).toBe("");
  await act(async () => root.setAttribute("data-theme", "graphite"));
  expect(container.innerHTML).toBe("");
  await act(async () => root.setAttribute("data-theme", "halloween"));
  await findByText((_, el) => el?.classList.contains("halloween-decor") ?? false);
  expect(container.querySelectorAll(".halloween-pumpkin")).toHaveLength(4);
  expect(container.querySelectorAll(".halloween-bat")).toHaveLength(3);
  expect(container.querySelectorAll(".halloween-web")).toHaveLength(1);
  expect(container.querySelectorAll(".halloween-spider")).toHaveLength(1);
  await act(async () => root.setAttribute("data-theme", "ocean"));
  await waitFor(() => expect(container.innerHTML).toBe(""));
});

test("T2.b: it is hidden from assistive tech, never takes a click, and stops under reduced motion", () => {
  root.setAttribute("data-theme", "halloween");
  const { container } = render(<HalloweenDecor />);
  expect(container.querySelector(".halloween-decor")?.getAttribute("aria-hidden")).toBe("true");
  const decorRule = css.match(/\.halloween-decor\s*\{[^}]*\}/)?.[0] ?? "";
  expect(decorRule).toContain("pointer-events: none");
  const reduce = css.match(/@media \(prefers-reduced-motion: reduce\) \{(?:[^{}]|\{[^{}]*\})*\.halloween-pumpkin(?:[^{}]|\{[^{}]*\})*\}/)?.[0] ?? "";
  expect(reduce).toContain(".halloween-bat");
  expect(reduce).toContain(".halloween-spider");
});

test("T2.c: the component names no colour of its own", () => {
  const src = readFileSync(resolve(__dirname, "HalloweenDecor.tsx"), "utf8");
  expect(src).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
  expect(src).not.toContain("rgb(");
});

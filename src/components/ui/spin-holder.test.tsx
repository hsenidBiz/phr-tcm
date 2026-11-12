// The holder turns, not the icon: a 16px box keeps the spin centre on a whole
// device pixel at fractional scaling (see spin-holder.tsx).

import { render } from "@testing-library/react";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import { IconRefresh } from "../../lib/actionIcons";
import { SpinHolder } from "./spin-holder";

test("while spinning the 16px holder turns and the icon inside does not", () => {
  const { container } = render(
    <SpinHolder spinning>
      <IconRefresh aria-hidden />
    </SpinHolder>,
  );
  const holder = container.firstElementChild as HTMLElement;
  expect(holder).toHaveClass("animate-spin", "size-4");
  expect(holder.querySelector("svg")).not.toHaveClass("animate-spin");
});

test("at rest nothing turns", () => {
  const { container } = render(
    <SpinHolder spinning={false}>
      <IconRefresh aria-hidden />
    </SpinHolder>,
  );
  expect(container.querySelector(".animate-spin")).toBeNull();
});

test("every spinner in the app turns through SpinHolder", () => {
  // A bare animate-spin on a 14px icon bobs at 150% scaling (see spin-holder.tsx).
  const src = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
  const offenders = (readdirSync(src, { recursive: true }) as string[])
    .filter((f) => f.endsWith(".tsx") && !f.includes(".test.") && !f.endsWith("spin-holder.tsx"))
    .filter((f) => readFileSync(join(src, f), "utf-8").includes("animate-spin"));
  expect(offenders).toEqual([]);
});

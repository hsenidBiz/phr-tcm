// The holder turns, not the icon: a 16px box keeps the spin centre on a whole
// device pixel at fractional scaling (see spin-holder.tsx).

import { render } from "@testing-library/react";
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

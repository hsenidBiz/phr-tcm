import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";

import ProjectChip from "./ProjectChip";

test("the chip shows the project name inside a trimmed label with a title", () => {
  render(<ProjectChip project="Payroll" />);
  const name = screen.getByText("Payroll");
  expect(name).toHaveClass("label-trim");
  expect(name.parentElement).toHaveAttribute("title", "Project: Payroll");
});

test("a screen reader hears what the chip is", () => {
  const { container } = render(<ProjectChip project="Payroll" />);
  expect(container.textContent).toBe("Project: Payroll");
  expect(screen.getByText("Project:")).toHaveClass("sr-only");
});

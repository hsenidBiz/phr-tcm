import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";

import ProjectChip from "./ProjectChip";

test("the chip shows the project name inside a trimmed label with a title", () => {
  render(<ProjectChip project="Payroll" />);
  const name = screen.getByText("Payroll");
  expect(name).toHaveClass("label-trim");
  expect(name.parentElement).toHaveAttribute("title", "Project: Payroll");
});

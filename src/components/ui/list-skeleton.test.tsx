import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { ListSkeleton } from "./list-skeleton";

test("a labelled skeleton is a status that says what is loading, its rows hidden", () => {
  render(<ListSkeleton label="Loading test points" rows={4} />);
  expect(screen.getByRole("status")).toHaveTextContent("Loading test points");
  const rows = screen.getAllByTestId("skeleton");
  expect(rows).toHaveLength(4);
  for (const r of rows) expect(r).toHaveAttribute("aria-hidden", "true");
});

test("an unlabelled skeleton is decoration only", () => {
  render(<ListSkeleton />);
  expect(screen.queryByRole("status")).toBeNull();
  expect(screen.getAllByTestId("skeleton")).toHaveLength(5);
});

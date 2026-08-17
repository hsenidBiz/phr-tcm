// The rail's badge bubbles: a count on an item when something arrived,
// nothing at all when the count is zero.

import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import Sidebar, { WORK_ITEMS, type WorkSection } from "./Sidebar";

test("a badge count lands on its item and reaches the accessible name", () => {
  render(
    <Sidebar<WorkSection>
      section="prs"
      onSelect={() => {}}
      items={WORK_ITEMS}
      badges={{ board: 3 }}
    />,
  );
  expect(screen.getByRole("button", { name: "Board (3 new)" })).toBeInTheDocument();
  expect(screen.getByText("3")).toBeInTheDocument();
});

test("zero and absent counts render no bubble", () => {
  render(
    <Sidebar<WorkSection>
      section="prs"
      onSelect={() => {}}
      items={WORK_ITEMS}
      badges={{ board: 0 }}
    />,
  );
  expect(screen.getByRole("button", { name: "Board" })).toBeInTheDocument();
  expect(screen.queryByText("0")).not.toBeInTheDocument();
});

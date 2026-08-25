import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import RecentPbis from "./RecentPbis";

afterEach(() => localStorage.clear());

function seed() {
  localStorage.setItem(
    "tcm-v2-recent-pbis:acme/Web",
    JSON.stringify([
      { id: 147359, title: "[PH] PAY - Reports", work_item_type: "Product Backlog Item" },
      { id: 138416, title: "Performance Assessment", work_item_type: "Product Backlog Item" },
    ]),
  );
}

test("a recent card picks on click and its X removes it instead", () => {
  seed();
  const onPick = vi.fn();
  render(<RecentPbis org="acme" project="Web" onPick={onPick} />);

  // The X removes without picking...
  fireEvent.click(screen.getByLabelText("Remove #147359 from recent PBIs"));
  expect(onPick).not.toHaveBeenCalled();
  expect(screen.queryByText(/PAY - Reports/)).not.toBeInTheDocument();
  expect(JSON.parse(localStorage.getItem("tcm-v2-recent-pbis:acme/Web")!)).toHaveLength(1);

  // ...and the surviving card still picks normally.
  fireEvent.click(screen.getByText("Performance Assessment"));
  expect(onPick).toHaveBeenCalledWith(
    expect.objectContaining({ id: 138416, title: "Performance Assessment" }),
  );
});

test("removing the last recent removes the whole section", () => {
  localStorage.setItem(
    "tcm-v2-recent-pbis:acme/Web",
    JSON.stringify([{ id: 1, title: "Only one", work_item_type: "Product Backlog Item" }]),
  );
  render(<RecentPbis org="acme" project="Web" onPick={() => {}} />);
  fireEvent.click(screen.getByLabelText("Remove #1 from recent PBIs"));
  expect(screen.queryByText("Recent PBIs")).not.toBeInTheDocument();
});

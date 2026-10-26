import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import RecentPbis from "./RecentPbis";
import { seedCombinedRecents } from "./PbiPicker";

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

describe("switched off", () => {
  beforeEach(() => localStorage.setItem("tcm-v2-all-projects", "0"));

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

  test("with no project picked nothing shows", () => {
    seed();
    render(<RecentPbis org="acme" project="" onPick={() => {}} />);
    expect(screen.queryByText("Recent PBIs")).not.toBeInTheDocument();
  });
});

describe("combined", () => {
  test("combined recents seed once from the home project with the project filled in", () => {
    seed();
    const old = localStorage.getItem("tcm-v2-recent-pbis:acme/Web");
    seedCombinedRecents("acme", "Web");

    const combined = JSON.parse(localStorage.getItem("tcm-v2-recent-pbis:acme/*")!) as Array<{
      id: number;
      project: string;
    }>;
    expect(combined.map((p) => p.id)).toEqual([147359, 138416]);
    expect(combined.every((p) => p.project === "Web")).toBe(true);
    // The old key survives, untouched.
    expect(localStorage.getItem("tcm-v2-recent-pbis:acme/Web")).toBe(old);

    // A second seed after the combined list was edited does not overwrite it.
    localStorage.setItem(
      "tcm-v2-recent-pbis:acme/*",
      JSON.stringify([{ id: 1, title: "Edited", work_item_type: "Product Backlog Item", project: "HRM" }]),
    );
    seedCombinedRecents("acme", "Web");
    const after = JSON.parse(localStorage.getItem("tcm-v2-recent-pbis:acme/*")!) as unknown[];
    expect(after).toHaveLength(1);
  });

  test("a recent that already names its project keeps it", () => {
    localStorage.setItem(
      "tcm-v2-recent-pbis:acme/Web",
      JSON.stringify([{ id: 5, title: "Kept", work_item_type: "Product Backlog Item", project: "HRM" }]),
    );
    seedCombinedRecents("acme", "Web");
    const combined = JSON.parse(localStorage.getItem("tcm-v2-recent-pbis:acme/*")!) as Array<{
      project: string;
    }>;
    expect(combined[0].project).toBe("HRM");
  });

  test("combined recents show with no project picked", () => {
    localStorage.setItem(
      "tcm-v2-recent-pbis:acme/*",
      JSON.stringify([
        { id: 9, title: "Org-wide one", work_item_type: "Product Backlog Item", project: "HRM" },
      ]),
    );
    render(<RecentPbis org="acme" project="" onPick={() => {}} />);
    expect(screen.getByText("Recent PBIs")).toBeInTheDocument();
    expect(screen.getByText("Org-wide one")).toBeInTheDocument();
  });

  test("combined recents name their project only when they span two", () => {
    localStorage.setItem(
      "tcm-v2-recent-pbis:acme/*",
      JSON.stringify([
        { id: 9, title: "One", work_item_type: "Product Backlog Item", project: "HRM" },
        { id: 10, title: "Two", work_item_type: "Product Backlog Item", project: "Payroll" },
      ]),
    );
    const { unmount } = render(<RecentPbis org="acme" project="" onPick={() => {}} />);
    expect(screen.getByText("HRM")).toBeInTheDocument();
    expect(screen.getByText("Payroll")).toBeInTheDocument();
    unmount();

    localStorage.setItem(
      "tcm-v2-recent-pbis:acme/*",
      JSON.stringify([
        { id: 9, title: "One", work_item_type: "Product Backlog Item", project: "HRM" },
        { id: 10, title: "Two", work_item_type: "Product Backlog Item", project: "HRM" },
      ]),
    );
    render(<RecentPbis org="acme" project="" onPick={() => {}} />);
    expect(screen.queryByText("HRM")).not.toBeInTheDocument();
  });
});

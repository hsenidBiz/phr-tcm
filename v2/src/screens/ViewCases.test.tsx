import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import ViewCases from "./ViewCases";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderView() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ViewCases
        org="acme"
        project="Web"
        pbi={{ id: 42, title: "Login flow", work_item_type: "Product Backlog Item" }}
      />
    </QueryClientProvider>,
  );
}

const caseA = {
  id: 201,
  title: "Login - valid",
  tags: "smoke",
  automation_status: "Planned",
  steps: [
    { action: "Open login page", expected: "Form shown" },
    { action: "Submit valid creds", expected: "Dashboard opens" },
  ],
  step_ids: ["2", "3"],
  module_value: "",
  preconditions: "User exists",
};
const caseB = { ...caseA, id: 202, title: "Login - locked out", tags: "" };
const caseC = { ...caseA, id: 203, title: "Checkout", tags: "" };

function mockCases(onView?: (queue: Array<{ update_id: number | null }>) => void) {
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [caseA, caseB, caseC];
    if (cmd === "view_queue_html") {
      onView?.((args as { queue: Array<{ update_id: number | null }> }).queue);
      return null;
    }
  });
}

test("rows start compact; the chevron expands steps, tags and the comment editor", async () => {
  mockCases();
  renderView();

  await screen.findByText("Login - valid");
  expect(screen.queryByText("Open login page")).not.toBeInTheDocument();
  // Tags stay out of the compact row - they only show in the detail.
  expect(screen.queryByText("smoke")).not.toBeInTheDocument();

  fireEvent.click(screen.getByLabelText("Expand #201"));
  expect(screen.getByText("Open login page")).toBeInTheDocument();
  expect(screen.getByText("Dashboard opens")).toBeInTheDocument();
  expect(screen.getByText(/User exists/)).toBeInTheDocument();
  expect(screen.getByText("smoke")).toBeInTheDocument();

  // Comment saves locally with a visible "Comment" chip on the row.
  fireEvent.click(screen.getByRole("button", { name: /Add comment/ }));
  fireEvent.change(screen.getByLabelText("Comment for #201"), {
    target: { value: "Step 2 needs the new MFA prompt" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save comment" }));
  const chip = screen.getByLabelText("Has a local comment");
  expect(chip).toHaveTextContent("Comment");
  expect(chip).toHaveAttribute("title", "Step 2 needs the new MFA prompt");

  // Clicking the chip opens a focused dialog with the comment (collapse
  // the row first so we prove the dialog is what shows it).
  fireEvent.click(screen.getByLabelText("Expand #201"));
  expect(screen.queryByText("Step 2 needs the new MFA prompt")).not.toBeInTheDocument();
  fireEvent.click(chip);
  const dialog = screen.getByRole("dialog", { name: "Comment for #201" });
  expect(dialog).toHaveTextContent("Step 2 needs the new MFA prompt");

  // Editing inside the dialog persists to the store.
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(screen.getByLabelText("Comment for #201 (edit)"), {
    target: { value: "Updated from the dialog" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save comment" }));
  expect(JSON.parse(localStorage.getItem("tcm-v2-case-notes:acme")!)["201"]).toBe(
    "Updated from the dialog",
  );

  fireEvent.click(screen.getByLabelText("Close comment"));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

  // Remove deletes the note (dialog closes, chip disappears, store empty).
  fireEvent.click(screen.getByLabelText("Has a local comment"));
  fireEvent.click(screen.getByRole("button", { name: "Remove" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.queryByLabelText("Has a local comment")).not.toBeInTheDocument();
  expect(JSON.parse(localStorage.getItem("tcm-v2-case-notes:acme")!)).toEqual({});
});

test("selection drives View in browser; nothing selected sends all visible", async () => {
  const sent: number[][] = [];
  mockCases((queue) => sent.push(queue.map((c) => c.update_id!)));
  renderView();

  // No selection: everything visible goes to the report.
  await screen.findByText("Login - valid");
  fireEvent.click(screen.getByRole("button", { name: "View in browser" }));
  await waitFor(() => expect(sent).toHaveLength(1));
  expect(sent[0]).toEqual([201, 202, 203]);

  // Click + shift-click selects a range; the button reflects the count.
  fireEvent.click(screen.getByText("Login - valid"));
  fireEvent.click(screen.getByText("Login - locked out"), { shiftKey: true });
  fireEvent.click(screen.getByRole("button", { name: "View 2 in browser" }));
  await waitFor(() => expect(sent).toHaveLength(2));
  expect(sent[1]).toEqual([201, 202]);
});

test("Group by title folds cases under shared prefixes and persists collapse", async () => {
  mockCases();
  renderView();
  await screen.findByText("Login - valid");

  fireEvent.click(screen.getByRole("checkbox"));
  const header = await screen.findByRole("button", { name: "Login (2)" });

  // Header click selects the group.
  fireEvent.click(header);
  expect(screen.getByText("2 selected")).toBeInTheDocument();

  // Chevron collapses; state lands in localStorage for the next session.
  fireEvent.click(screen.getByLabelText("Collapse group Login"));
  expect(screen.queryByText("Login - valid")).not.toBeInTheDocument();
  expect(screen.getByText("Checkout")).toBeInTheDocument();
  expect(JSON.parse(localStorage.getItem("tcm-v2-view-collapsed-groups")!)).toEqual(["Login"]);
});

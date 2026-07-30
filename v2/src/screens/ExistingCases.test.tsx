import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import ExistingCases from "./ExistingCases";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderCases() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ExistingCases org="acme" project="Web" pbiId={42} />
    </QueryClientProvider>,
  );
}

const fullCase = {
  id: 201,
  title: "Valid login",
  tags: "smoke",
  automation_status: "Planned",
  steps: [
    { action: "Open page", expected: "Shown" },
    { action: "Submit", expected: "" },
  ],
  step_ids: ["2", "3"],
  module_value: "",
  preconditions: "",
};

const secondCase = { ...fullCase, id: 202, title: "Invalid login" };

test("expands a case via the chevron and saves edits via update_test_case", async () => {
  let updated: { tc?: { title: string; update_id: number | null } } = {};
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "update_test_case") {
      updated = args as typeof updated;
      return null;
    }
  });
  renderCases();

  await screen.findByText("Valid login");
  fireEvent.click(screen.getByLabelText("Expand #201"));
  const titleInput = await screen.findByLabelText("Case title");
  fireEvent.change(titleInput, { target: { value: "Valid login v2" } });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

  await screen.findByText("Valid login"); // list still rendered
  expect(updated.tc?.title).toBe("Valid login v2");
  expect(updated.tc?.update_id).toBe(201);
});

test("invalid edits disable save with a reason", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
  });
  renderCases();
  await screen.findByText("Valid login");
  fireEvent.click(screen.getByLabelText("Expand #201"));
  const titleInput = await screen.findByLabelText("Case title");
  fireEvent.change(titleInput, { target: { value: "   " } });
  expect(screen.getByRole("button", { name: "Save changes" })).toBeDisabled();
  expect(screen.getByText(/Title is required/)).toBeInTheDocument();
});

test("group-header click selects every case in the group", async () => {
  localStorage.setItem("tcm-v2-group-cases", "on");
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full")
      return [
        { ...fullCase, id: 201, title: "Login - valid" },
        { ...fullCase, id: 202, title: "Login - locked out" },
        { ...fullCase, id: 203, title: "Standalone thing" },
      ];
  });
  renderCases();

  const header = await screen.findByRole("button", { name: "Login (2)" });
  fireEvent.click(header);
  expect(screen.getByText("2 selected")).toBeInTheDocument();

  // Clicking again clears the group's selection.
  fireEvent.click(header);
  expect(screen.queryByText("2 selected")).not.toBeInTheDocument();

  // The chevron collapses the group's cards (header stays).
  fireEvent.click(screen.getByLabelText("Collapse group Login"));
  expect(screen.queryByText("Login - valid")).not.toBeInTheDocument();
  expect(screen.getByText("Standalone thing")).toBeInTheDocument();
  fireEvent.click(screen.getByLabelText("Expand group Login"));
  expect(screen.getByText("Login - valid")).toBeInTheDocument();
});

test("card clicks drive multi-select and unlock the bulk toolbar", async () => {
  const updatedIds: number[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase, secondCase];
    if (cmd === "update_test_case") {
      const a = args as { tc: { update_id: number | null } };
      if (a.tc.update_id != null) updatedIds.push(a.tc.update_id);
      return null;
    }
  });
  renderCases();

  // Single click selects one; ctrl+click adds the second.
  fireEvent.click(await screen.findByText("Valid login"));
  expect(screen.getByText("1 selected")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Invalid login"), { ctrlKey: true });
  expect(screen.getByText("2 selected")).toBeInTheDocument();

  // Bulk edit both: pick a status, apply serially.
  fireEvent.click(screen.getByRole("button", { name: "Bulk edit" }));
  fireEvent.change(screen.getByLabelText(/Automation status/), {
    target: { value: "Not Automated" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Apply to 2" }));
  await waitFor(() => expect(updatedIds).toHaveLength(2));
  expect([...updatedIds].sort()).toEqual([201, 202]);
});

/** Backing out of a bulk dialog must not undo the work of choosing what to
 *  bulk-edit. Power Rename's close handler used to clear the selection, so
 *  Cancel left the user re-picking every case to try again. */
test("cancelling Power Rename keeps the selection", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase, secondCase];
    if (cmd === "list_project_tags") return [];
  });
  renderCases();

  fireEvent.click(await screen.findByText("Valid login"));
  fireEvent.click(screen.getByText("Invalid login"), { ctrlKey: true });
  expect(screen.getByText("2 selected")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Power Rename" }));
  fireEvent.click(await screen.findByRole("button", { name: /Cancel/ }));

  expect(screen.getByText("2 selected")).toBeInTheDocument();
  // And the dialog really did close, so this is not just a stale render.
  expect(screen.queryByRole("button", { name: /Cancel/ })).not.toBeInTheDocument();
});

test("search narrows the list by title, id or tag", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full")
      return [fullCase, secondCase, { ...fullCase, id: 203, title: "Checkout", tags: "regression" }];
  });
  renderCases();
  await screen.findByText("Valid login");

  const box = screen.getByLabelText("Search test cases");

  // Title match: only the checkout case stays.
  fireEvent.change(box, { target: { value: "checkout" } });
  expect(screen.queryByText("Valid login")).not.toBeInTheDocument();
  expect(screen.getByText("Checkout")).toBeInTheDocument();

  // Id match.
  fireEvent.change(box, { target: { value: "#202" } });
  expect(screen.getByText("Invalid login")).toBeInTheDocument();
  expect(screen.queryByText("Checkout")).not.toBeInTheDocument();

  // Tag match.
  fireEvent.change(box, { target: { value: "regression" } });
  expect(screen.getByText("Checkout")).toBeInTheDocument();

  // No match shows the empty hint; clearing restores everything.
  fireEvent.change(box, { target: { value: "zzz" } });
  expect(screen.getByText(/No test cases match/)).toBeInTheDocument();
  fireEvent.change(box, { target: { value: "" } });
  expect(screen.getByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("Checkout")).toBeInTheDocument();
});

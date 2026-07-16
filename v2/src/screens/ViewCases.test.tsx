import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
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

function mockCases() {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full")
      return [
        {
          id: 201,
          title: "Valid login",
          tags: "smoke",
          automation_status: "Planned",
          steps: [
            { action: "Open login page", expected: "Form shown" },
            { action: "Submit valid creds", expected: "Dashboard opens" },
          ],
          step_ids: ["2", "3"],
          module_value: "",
          preconditions: "User exists",
        },
      ];
  });
}

test("shows every case fully expanded: steps, preconditions, tags", async () => {
  mockCases();
  renderView();

  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("Open login page")).toBeInTheDocument();
  expect(screen.getByText("Dashboard opens")).toBeInTheDocument();
  expect(screen.getByText(/User exists/)).toBeInTheDocument();
  expect(screen.getByText(/2 steps · Planned · smoke/)).toBeInTheDocument();
});

test("a comment saves locally and survives a remount", async () => {
  mockCases();
  const first = renderView();

  fireEvent.click(await screen.findByRole("button", { name: /Add comment/ }));
  fireEvent.change(screen.getByLabelText("Comment for #201"), {
    target: { value: "Step 2 needs the new MFA prompt" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save comment" }));
  expect(screen.getByText("Step 2 needs the new MFA prompt")).toBeInTheDocument();

  // Fresh mount (new session): the comment comes back from localStorage.
  first.unmount();
  renderView();
  expect(await screen.findByText("Step 2 needs the new MFA prompt")).toBeInTheDocument();
  // And it can be edited from the saved state.
  expect(screen.getByRole("button", { name: "Edit" })).toBeInTheDocument();
});

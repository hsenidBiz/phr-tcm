import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import Browse from "./Browse";

afterEach(() => clearMocks());

function renderBrowse(org = "acme", project = "Web") {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Browse org={org} project={project} />
    </QueryClientProvider>,
  );
}

function mockAll() {
  mockIPC((cmd, args) => {
    const a = args as Record<string, unknown>;
    switch (cmd) {
      case "search_pbis":
        return a.project === "Web" && a.query === "login"
          ? [{ id: 42, title: "Login flow", work_item_type: "Product Backlog Item" }]
          : [];
      case "pbi_test_cases":
        return a.pbiId === 42
          ? [{ id: 201, title: "Valid login", tags: "smoke", automation_status: "Planned" }]
          : [];
      case "pbi_test_cases_full":
        return a.pbiId === 42
          ? [{
              id: 201,
              title: "Valid login",
              tags: "smoke",
              automation_status: "Planned",
              steps: [{ action: "Open", expected: "Shown" }],
              module_value: "",
              preconditions: "",
            }]
          : [];
      case "list_test_case_fields":
        return [];
      case "ensure_pbi_suite":
        return { plan_id: 9, plan_name: "Plan", suite_id: 91 };
      case "list_test_points":
        return [];
    }
  });
}

test("search -> pick PBI -> linked cases render in the editor", async () => {
  mockAll();
  renderBrowse();

  const search = screen.getByPlaceholderText("Title or ID");
  fireEvent.change(search, { target: { value: "login" } });
  fireEvent.keyDown(search, { key: "Enter" });
  const hit = await screen.findByText(/Login flow/);

  fireEvent.click(hit);
  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText(/1 steps · Planned/)).toBeInTheDocument();
});

test("empty search result shows a friendly message", async () => {
  mockAll();
  renderBrowse();
  const search = screen.getByPlaceholderText("Title or ID");
  fireEvent.change(search, { target: { value: "nothing" } });
  fireEvent.keyDown(search, { key: "Enter" });
  expect(await screen.findByText(/No PBIs match "nothing"/)).toBeInTheDocument();
});

test("prompts for scope when org/project missing", () => {
  mockAll();
  renderBrowse("", "");
  expect(screen.getByText(/Pick an organization and project/)).toBeInTheDocument();
});

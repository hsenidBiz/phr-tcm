import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import ImportFile from "./ImportFile";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const pbi = { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" };

function renderScreen() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ImportFile org="acme" project="Web" pbi={pbi} />
    </QueryClientProvider>,
  );
}

test("import feeds the shared queue; failed items stay queued", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "plugin:dialog|open") return "C:\\cases.json";
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "parse_import_file")
      return {
        cases: [
          { title: "Good", steps: [{ action: "A", expected: "" }], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null },
          { title: "Bad", steps: [{ action: "B", expected: "" }], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null },
        ],
        warnings: ["Row 9: something odd"],
      };
    if (cmd === "submit_queue") {
      const a = args as { queue: Array<{ title: string }> };
      return a.queue.map((tc, index) => ({
        index,
        title: tc.title,
        action: tc.title === "Bad" ? "failed" : "created",
        id: tc.title === "Bad" ? null : 901,
        error: tc.title === "Bad" ? "boom" : null,
      }));
    }
  });
  renderScreen();
  fireEvent.click(screen.getByRole("button", { name: "Import JSON" }));
  await screen.findByText("Good");
  expect(screen.getByText("Row 9: something odd")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /Review 2 test cases/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Confirm & create 2/ }));
  fireEvent.click(screen.getByRole("button", { name: /Yes — create 2/ }));
  expect(await screen.findByText(/Failed: Bad - boom/)).toBeInTheDocument();
  expect(screen.getByText(/1 queued/)).toBeInTheDocument();
});

test("Generate AI guide opens the wizard", async () => {
  mockIPC((cmd) => {
    if (cmd === "test_case_field_values") return [];
    if (cmd === "list_project_tags") return [];
  });
  renderScreen();
  fireEvent.click(screen.getByRole("button", { name: "Generate AI guide…" }));
  expect(await screen.findByText("AI test-case guide")).toBeInTheDocument();
});

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import ManualEntry from "./ManualEntry";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const pbi = { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" };

function renderScreen(pbiArg: typeof pbi | null = pbi) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ManualEntry org="acme" project="Web" pbi={pbiArg} />
    </QueryClientProvider>,
  );
}

function baseMocks(handler: (cmd: string, args: unknown) => unknown = () => undefined) {
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return ["smoke", "regression"];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [{ id: 201, title: "Existing case", tags: "", automation_status: "Planned" }];
    return handler(cmd, args);
  });
}

function addCase(title: string) {
  fireEvent.change(screen.getByPlaceholderText("Test case title"), {
    target: { value: title },
  });
  // Fill step 1 via the shared StepsEditor grid, add + fill step 2.
  fireEvent.change(screen.getByLabelText("Step 1 action"), {
    target: { value: "Open page" },
  });
  fireEvent.change(screen.getByLabelText("Step 1 expected"), {
    target: { value: "Page shown" },
  });
  fireEvent.click(screen.getByRole("button", { name: "+ Add Step" }));
  fireEvent.change(screen.getByLabelText("Step 2 action"), {
    target: { value: "Submit form" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add to queue" }));
}

test("prompts for scope when no PBI is chosen", () => {
  baseMocks();
  renderScreen(null);
  expect(screen.getByText(/Pick an organization, project and PBI/)).toBeInTheDocument();
});

test("manual add, review gate, submit reports results", async () => {
  baseMocks((cmd, args) => {
    if (cmd === "submit_queue") {
      const a = args as { queue: Array<{ title: string }> };
      return a.queue.map((tc, index) => ({
        index,
        title: tc.title,
        action: "created",
        id: 900 + index,
        error: null,
      }));
    }
  });
  renderScreen();
  addCase("Login works");
  expect(await screen.findByText("Login works")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  // Two-stage confirm: arming shows the check-the-PBI warning, then the
  // explicit Yes actually writes.
  fireEvent.click(await screen.findByRole("button", { name: /Confirm & create 1/ }));
  expect(screen.getByText(/cannot be deleted/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /Yes — create 1/ }));
  expect(await screen.findByText(/Created #900: Login works/)).toBeInTheDocument();

  // Clear results returns the screen to normal and removes itself.
  fireEvent.click(screen.getByRole("button", { name: "Clear results" }));
  expect(screen.queryByText(/Created #900/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Clear results" })).not.toBeInTheDocument();
});

test("duplicate titles warn in review but do not block", async () => {
  baseMocks();
  renderScreen();
  addCase("Existing case");
  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  expect(await screen.findByText(/create a duplicate/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Confirm & create 1/ })).toBeEnabled();
});

test("draft queue persists across remounts (shared with Import File)", async () => {
  baseMocks();
  const first = renderScreen();
  addCase("Persistent case");
  await screen.findByText("Persistent case");
  first.unmount();

  renderScreen();
  expect(await screen.findByText("Persistent case")).toBeInTheDocument();
});

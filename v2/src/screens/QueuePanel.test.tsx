import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import QueuePanel from "./QueuePanel";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderPanel(existingTitles: string[] = []) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <QueuePanel org="acme" project="Web" pbiId={42} existingTitles={existingTitles} />
    </QueryClientProvider>,
  );
}

function baseMocks(handler: (cmd: string, args: unknown) => unknown) {
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    return handler(cmd, args);
  });
}

function addCase(title: string) {
  fireEvent.change(screen.getByPlaceholderText("Test case title"), {
    target: { value: title },
  });
  fireEvent.change(screen.getByPlaceholderText(/One step per line/), {
    target: { value: "Open page => Page shown\nSubmit form" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add to queue" }));
}

test("manual add, review gate, then submit reports results", async () => {
  let refs: { moduleRef?: unknown; preconditionsRef?: unknown } = {};
  baseMocks((cmd, args) => {
    if (cmd === "submit_queue") {
      const a = args as {
        queue: Array<{ title: string }>;
        moduleRef: unknown;
        preconditionsRef: unknown;
      };
      refs = { moduleRef: a.moduleRef, preconditionsRef: a.preconditionsRef };
      return a.queue.map((tc, index) => ({
        index,
        title: tc.title,
        action: "created",
        id: 900 + index,
        error: null,
      }));
    }
  });
  renderPanel();
  addCase("Login works");
  expect(await screen.findByText("Login works")).toBeInTheDocument();

  // Review gate first, then confirm.
  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Confirm & create 1/ }));

  expect(await screen.findByText(/Created #900: Login works/)).toBeInTheDocument();
  expect(refs).toEqual({ moduleRef: null, preconditionsRef: null });
});

test("duplicate titles warn in review but do not block", async () => {
  baseMocks(() => undefined);
  renderPanel(["Login works"]);
  addCase("Login works");
  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  expect(await screen.findByText(/create a duplicate/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Confirm & create 1/ })).toBeEnabled();
});

test("draft queue persists per PBI across remounts", async () => {
  baseMocks(() => undefined);
  const first = renderPanel();
  addCase("Persistent case");
  await screen.findByText("Persistent case");
  first.unmount();

  renderPanel();
  expect(await screen.findByText("Persistent case")).toBeInTheDocument();
  expect(screen.getByText(/1 queued/)).toBeInTheDocument();
});

test("failed submit items stay in the queue for retry", async () => {
  baseMocks((cmd, args) => {
    if (cmd === "plugin:dialog|open") return "C:\\cases.xlsx";
    if (cmd === "parse_import_file")
      return {
        cases: [
          { title: "Good", steps: [{ action: "A", expected: "" }], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null },
          { title: "Bad", steps: [{ action: "B", expected: "" }], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null },
        ],
        warnings: [],
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
  renderPanel();
  fireEvent.click(screen.getByRole("button", { name: "Import file..." }));
  await screen.findByText("Good");
  fireEvent.click(screen.getByRole("button", { name: /Review 2 test cases/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Confirm & create 2/ }));

  expect(await screen.findByText(/Failed: Bad - boom/)).toBeInTheDocument();
  expect(screen.getByText(/Queue for PBI #42 \(1 queued\)/)).toBeInTheDocument();
});

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import QueuePanel from "./QueuePanel";

afterEach(() => clearMocks());

function renderPanel() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <QueuePanel org="acme" project="Web" pbiId={42} />
    </QueryClientProvider>,
  );
}

test("manual add builds queue and submit reports results", async () => {
  mockIPC((cmd, args) => {
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
  renderPanel();

  fireEvent.change(screen.getByPlaceholderText("Test case title"), {
    target: { value: "Login works" },
  });
  fireEvent.change(screen.getByPlaceholderText(/One step per line/), {
    target: { value: "Open page => Page shown\nSubmit form" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add to queue" }));

  expect(await screen.findByText("Login works")).toBeInTheDocument();
  expect(screen.getByText("2 steps")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /Create 1 test case/ }));
  expect(await screen.findByText(/Created #900: Login works/)).toBeInTheDocument();
});

test("import merges parsed cases and shows warnings", async () => {
  mockIPC((cmd) => {
    if (cmd === "plugin:dialog|open") return "C:\\cases.xlsx";
    if (cmd === "parse_import_file")
      return {
        cases: [
          {
            title: "Imported case",
            steps: [{ action: "Do", expected: "" }],
            tags: "",
            automation_status: "Not Automated",
            module_value: "",
            preconditions: "",
            update_id: 123,
          },
        ],
        warnings: ["Row 9: something odd"],
      };
  });
  renderPanel();
  fireEvent.click(screen.getByRole("button", { name: "Import file..." }));
  expect(await screen.findByText("Imported case")).toBeInTheDocument();
  expect(screen.getByText("UPDATE #123")).toBeInTheDocument();
  expect(screen.getByText("Row 9: something odd")).toBeInTheDocument();
});

test("failed submit items stay in the queue for retry", async () => {
  mockIPC((cmd, args) => {
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
  fireEvent.click(screen.getByRole("button", { name: /Create 2 test cases/ }));

  expect(await screen.findByText(/Failed: Bad - boom/)).toBeInTheDocument();
  // Good was created and removed from the queue; Bad remains queued.
  expect(screen.getByText(/Queue for PBI #42 \(1 queued\)/)).toBeInTheDocument();
});

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import ImportFile from "./ImportFile";
import { Toaster } from "sonner";

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

/** Mirrors App: the PBI lives above ImportFile, so onPickPbi genuinely
 * re-keys useQueue - the condition the switch path has to survive. */
function StatefulHost({ initial }: { initial: typeof pbi }) {
  const [current, setCurrent] = useState(initial);
  return (
    <>
      <span data-testid="current-pbi">{current.id}</span>
      <ImportFile org="acme" project="Web" pbi={current} onPickPbi={setCurrent} />
    </>
  );
}

function renderHosted(initial = pbi) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <StatefulHost initial={initial} />
    </QueryClientProvider>,
  );
}

const sharedFor = (pbiId: number) => ({
  pbi_id: pbiId,
  pbi_title: "Timeline - split weight",
  pbi_work_item_type: "Product Backlog Item",
  organization: "acme",
  project: "Web",
  cases: [
    {
      update_id: null, title: "Shared case", tags: "", automation_status: "Not Automated",
      module_value: "", preconditions: "", comment: "",
      steps: [{ action: "a", expected: "b" }],
    },
  ],
  warnings: [],
});

test("import feeds the shared queue; failed items stay queued", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "plugin:dialog|open") return "C:\\cases.json";
    if (cmd === "list_repos") return [];
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

test("a matching PBI imports straight into the queue", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_shared_queue") return sharedFor(42); // same as selected
  });
  renderHosted();
  fireEvent.change(screen.getByLabelText("Share link"), {
    target: { value: "tcm-share:acme/Web/42/aaaa-1111" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Import shared" }));

  expect(await screen.findByText("Shared case")).toBeInTheDocument();
  // No question asked when there is nothing to choose between.
  expect(screen.queryByText("This draft is for a different PBI")).not.toBeInTheDocument();
});

test("a different PBI asks first, and Switch loads into THAT PBI's queue", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_shared_queue") return sharedFor(9999); // not the selected 42
  });
  renderHosted();
  fireEvent.change(screen.getByLabelText("Share link"), {
    target: { value: "tcm-share:acme/Web/9999/aaaa-1111" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Import shared" }));

  // Nothing is loaded until the user chooses.
  expect(await screen.findByText("This draft is for a different PBI")).toBeInTheDocument();
  expect(screen.queryByText("Shared case")).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Switch to #9999" }));

  // The PBI actually changed, and the cases landed AFTER the switch - so
  // they live in 9999's queue and survive being there (the reported bug
  // was them vanishing on switch because they went to the old queue).
  expect(await screen.findByText("Shared case")).toBeInTheDocument();
  expect(screen.getByTestId("current-pbi")).toHaveTextContent("9999");
  expect(localStorage.getItem("tcm-v2-draft:acme/9999")).toContain("Shared case");
  expect(localStorage.getItem("tcm-v2-draft:acme/42")).toBeNull();
});

test("Stay keeps the current PBI and warns about the mismatch", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_shared_queue") return sharedFor(9999);
  });
  renderHosted();
  fireEvent.change(screen.getByLabelText("Share link"), {
    target: { value: "tcm-share:acme/Web/9999/aaaa-1111" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Import shared" }));
  fireEvent.click(await screen.findByRole("button", { name: "Stay on #42" }));

  expect(await screen.findByText("Shared case")).toBeInTheDocument();
  expect(screen.getByTestId("current-pbi")).toHaveTextContent("42");
  expect(screen.getByText(/shared for PBI #9999/)).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-draft:acme/42")).toContain("Shared case");
});

test("Cancel loads nothing anywhere", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_shared_queue") return sharedFor(9999);
  });
  renderHosted();
  fireEvent.change(screen.getByLabelText("Share link"), {
    target: { value: "tcm-share:acme/Web/9999/aaaa-1111" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Import shared" }));
  fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));

  expect(screen.queryByText("Shared case")).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-draft:acme/42")).toBeNull();
  expect(localStorage.getItem("tcm-v2-draft:acme/9999")).toBeNull();
});

test("a spent share link shows the one-time-use explanation", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_shared_queue")
      throw "This share link has already been used, or was revoked by the sender.";
  });
  // The error surfaces as a toast - mount a Toaster alongside the screen.
  renderScreen();
  render(<Toaster />);
  fireEvent.change(screen.getByLabelText("Share link"), {
    target: { value: "tcm-share:acme/Web/1/aaaa" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Import shared" }));
  // Surfaced via toast; the queue stays empty.
  expect(await screen.findByText(/already been used/)).toBeInTheDocument();
});

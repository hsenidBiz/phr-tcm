import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import type { TestCase } from "../bindings";
import QueueSection from "./QueueSection";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function makeCase(overrides: Partial<TestCase> = {}): TestCase {
  return {
    title: "Login works",
    steps: [{ action: "Open page", expected: "Page shown" }],
    tags: "smoke",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
    update_id: null,
    ...overrides,
  };
}

/** Owns the queue state the way ManualEntry / ImportFile do. */
function Harness({ initial }: { initial: TestCase[] }) {
  const [queue, setQueue] = useState<TestCase[]>(initial);
  return <QueueSection org="acme" project="Web" pbiId={42} queue={queue} setQueue={setQueue} />;
}

function renderQueue(initial: TestCase[]) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Harness initial={initial} />
    </QueryClientProvider>,
  );
}

function baseMocks() {
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return ["smoke", "regression"];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    return undefined;
  });
}

test("Edit opens the inline editor and Save writes back into the queue", async () => {
  baseMocks();
  renderQueue([makeCase()]);

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const title = await screen.findByLabelText("Case title");
  fireEvent.change(title, { target: { value: "Login works — edited" } });
  fireEvent.change(screen.getByLabelText("Step 1 expected"), {
    target: { value: "Dashboard shown" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save to queue" }));

  // The row shows the new title and the editor is gone.
  expect(await screen.findByText("Login works — edited")).toBeInTheDocument();
  expect(screen.queryByLabelText("Case title")).not.toBeInTheDocument();
});

test("editing preserves the UPDATE badge (update_id survives a save)", async () => {
  baseMocks();
  renderQueue([makeCase({ update_id: 777 })]);
  expect(screen.getByText("UPDATE #777")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(await screen.findByLabelText("Case title"), {
    target: { value: "Renamed update" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save to queue" }));

  expect(await screen.findByText("Renamed update")).toBeInTheDocument();
  expect(screen.getByText("UPDATE #777")).toBeInTheDocument();
});

test("Save is blocked while the edited case is invalid", async () => {
  baseMocks();
  renderQueue([makeCase()]);

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(await screen.findByLabelText("Case title"), { target: { value: "  " } });

  expect(screen.getByRole("button", { name: "Save to queue" })).toBeDisabled();
  expect(screen.getByText("Title is required.")).toBeInTheDocument();
});

test("Cancel discards the edits", async () => {
  baseMocks();
  renderQueue([makeCase()]);

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(await screen.findByLabelText("Case title"), {
    target: { value: "Should not stick" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

  expect(screen.getByText("Login works")).toBeInTheDocument();
  expect(screen.queryByText("Should not stick")).not.toBeInTheDocument();
});

test("removing a row closes any open editor (indices shift)", async () => {
  baseMocks();
  renderQueue([makeCase(), makeCase({ title: "Second case" })]);

  fireEvent.click(screen.getAllByRole("button", { name: "Edit" })[1]);
  await screen.findByLabelText("Case title");
  fireEvent.click(screen.getAllByRole("button", { name: "Remove" })[0]);

  expect(screen.queryByLabelText("Case title")).not.toBeInTheDocument();
  expect(screen.getByText("Second case")).toBeInTheDocument();
});

test("a case's in-app comment shows on the row and is editable in the editor", async () => {
  baseMocks();
  renderQueue([makeCase({ comment: "Imported from sprint 12 sheet" })]);

  // The note from the JSON file renders on the queue row.
  expect(screen.getByText("Imported from sprint 12 sheet")).toBeInTheDocument();

  // The inline editor exposes it as an in-app-only field.
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const comment = await screen.findByLabelText("Comment (in-app only)");
  fireEvent.change(comment, { target: { value: "Re-check with QA" } });
  fireEvent.click(screen.getByRole("button", { name: "Save to queue" }));

  expect(await screen.findByText("Re-check with QA")).toBeInTheDocument();
  expect(screen.queryByText("Imported from sprint 12 sheet")).not.toBeInTheDocument();
});

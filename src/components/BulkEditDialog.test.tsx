// Bulk edit of real work items: no checkbox gates a field. Every field shows
// at once and starts at "Leave unchanged"; Apply stays dead until one is set;
// a line above the buttons names what will change.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { TestCaseFull } from "../bindings";
import BulkEditDialog from "./BulkEditDialog";

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.restoreAllMocks();
});

const fullCase = (id: number, over: Partial<TestCaseFull> = {}) =>
  ({
    id,
    title: `Case ${id}`,
    tags: "smoke",
    automation_status: "Planned",
    steps: [{ action: "Open", expected: "Shown" }],
    step_ids: ["2"],
    steps_xml: "<steps/>",
    priority: 3,
    module_value: "Payroll",
    preconditions: "Old precondition",
    ...over,
  }) as unknown as TestCaseFull;

type Sent = { tc: Record<string, unknown> };

function mount() {
  const sent: Sent["tc"][] = [];
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return ["Billing", "Payroll"];
    if (cmd === "update_test_case") {
      sent.push((args as Sent).tc);
      return null;
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onDone = vi.fn();
  render(
    <QueryClientProvider client={qc}>
      <BulkEditDialog
        org="acme"
        project="Web"
        cases={[fullCase(1), fullCase(2)]}
        onClose={() => {}}
        onDone={onDone}
      />
    </QueryClientProvider>,
  );
  return { sent, onDone };
}

const apply = () => screen.getByRole("button", { name: /^Apply to 2$/ });

async function pickModule(value: string) {
  fireEvent.click(screen.getByRole("combobox", { name: "Module" }));
  fireEvent.change(screen.getByPlaceholderText("Search…"), { target: { value } });
  fireEvent.click(await screen.findByRole("button", { name: `Use "${value}"` }));
}

function pickOption(selectName: string | RegExp, option: string) {
  fireEvent.click(screen.getByRole("combobox", { name: selectName }));
  fireEvent.click(screen.getByRole("option", { name: option }));
}

test("there are no checkboxes: every field shows at once and starts on Leave unchanged", () => {
  mount();
  expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
  expect(screen.queryByText("Set module")).not.toBeInTheDocument();
  expect(screen.queryByText("Set preconditions")).not.toBeInTheDocument();

  for (const name of ["Automation status", "Priority", "Tags", "Preconditions"]) {
    expect(screen.getByRole("combobox", { name })).toHaveTextContent("Leave unchanged");
  }
  // The module picker is always there; empty reads as unchanged.
  expect(screen.getByRole("combobox", { name: "Module" })).toHaveTextContent("Leave unchanged");
  // The replacement box only shows once Replace with is chosen.
  expect(screen.queryByRole("textbox", { name: "Preconditions text" })).not.toBeInTheDocument();

  expect(screen.getByText("Nothing to change yet")).toBeInTheDocument();
  expect(apply()).toBeDisabled();
});

test("picking a module enables Apply, names it, and clearing it goes back to unchanged", async () => {
  mount();
  await pickModule("Billing");
  expect(screen.getByText("Will change: Module")).toBeInTheDocument();
  expect(apply()).toBeEnabled();

  fireEvent.click(within(screen.getByRole("combobox", { name: "Module" })).getByRole("button", { name: "Clear" }));
  expect(screen.getByText("Nothing to change yet")).toBeInTheDocument();
  expect(apply()).toBeDisabled();
});

test("the summary lists every field that is set, in the order they appear", async () => {
  mount();
  await pickModule("Billing");
  pickOption("Priority", "Priority 1");
  expect(screen.getByText("Will change: Priority, Module")).toBeInTheDocument();
  pickOption("Automation status", "Planned");
  expect(screen.getByText("Will change: Automation status, Priority, Module")).toBeInTheDocument();
});

test("Apply sends the module and leaves every other field as each case had it", async () => {
  const { sent, onDone } = mount();
  await pickModule("Billing");
  fireEvent.click(apply());
  await waitFor(() => expect(onDone).toHaveBeenCalled());
  expect(sent).toHaveLength(2);
  for (const tc of sent) {
    expect(tc).toMatchObject({
      module_value: "Billing",
      preconditions: "Old precondition",
      automation_status: "Planned",
      tags: "smoke",
      priority: null,
    });
  }
});

test("Preconditions: Replace with shows a box and saves its text", async () => {
  const { sent, onDone } = mount();
  pickOption("Preconditions", "Replace with");
  const box = screen.getByRole("textbox", { name: "Preconditions text" });
  // Replace with nothing typed is not a change yet (that is what Clear is for).
  expect(apply()).toBeDisabled();
  fireEvent.change(box, { target: { value: "Signed in as admin" } });
  expect(screen.getByText("Will change: Preconditions")).toBeInTheDocument();
  fireEvent.click(apply());
  await waitFor(() => expect(onDone).toHaveBeenCalled());
  expect(sent.map((t) => t.preconditions)).toEqual(["Signed in as admin", "Signed in as admin"]);
});

test("Preconditions: Clear saves an empty text, and Leave unchanged goes back", async () => {
  const { sent, onDone } = mount();
  pickOption("Preconditions", "Clear");
  expect(screen.queryByRole("textbox", { name: "Preconditions text" })).not.toBeInTheDocument();
  expect(screen.getByText("Will change: Preconditions")).toBeInTheDocument();

  pickOption("Preconditions", "Leave unchanged");
  expect(apply()).toBeDisabled();

  pickOption("Preconditions", "Clear");
  fireEvent.click(apply());
  await waitFor(() => expect(onDone).toHaveBeenCalled());
  expect(sent.map((t) => t.preconditions)).toEqual(["", ""]);
});

test("Priority and Automation status still apply from their selects", async () => {
  const { sent, onDone } = mount();
  pickOption("Priority", "Priority 1");
  pickOption("Automation status", "Not Automated");
  fireEvent.click(apply());
  await waitFor(() => expect(onDone).toHaveBeenCalled());
  for (const tc of sent) {
    expect(tc).toMatchObject({ priority: 1, automation_status: "Not Automated", module_value: "Payroll" });
  }
});

test("Clear module clears the module on every case, and picking a module un-presses it", async () => {
  const { sent, onDone } = mount();
  const clear = screen.getByRole("button", { name: "Clear module" });
  expect(clear).toHaveAttribute("aria-pressed", "false");

  fireEvent.click(clear);
  expect(clear).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByText("Will change: Module (clear)")).toBeInTheDocument();
  expect(apply()).toBeEnabled();

  // Picking a module lets go of Clear: the pick is what is sent.
  await pickModule("Billing");
  expect(clear).toHaveAttribute("aria-pressed", "false");
  expect(screen.getByText("Will change: Module")).toBeInTheDocument();

  // Pressed again, then pressed off, it is back to unchanged.
  fireEvent.click(clear);
  fireEvent.click(clear);
  expect(screen.getByText("Nothing to change yet")).toBeInTheDocument();
  expect(apply()).toBeDisabled();

  fireEvent.click(clear);
  fireEvent.click(apply());
  await waitFor(() => expect(onDone).toHaveBeenCalled());
  expect(sent.map((t) => t.module_value)).toEqual(["", ""]);
});

test("Tags: Add with nothing typed is no change, and Replace with nothing reads as a clear", async () => {
  const { sent, onDone } = mount();
  pickOption("Tags", "Add tags");
  expect(screen.getByText("Nothing to change yet")).toBeInTheDocument();
  expect(apply()).toBeDisabled();

  pickOption("Tags", "Replace tags");
  expect(screen.getByText("Will change: Tags (clear)")).toBeInTheDocument();
  expect(apply()).toBeEnabled();
  fireEvent.click(apply());
  await waitFor(() => expect(onDone).toHaveBeenCalled());
  expect(sent.map((t) => t.tags)).toEqual(["", ""]);
});

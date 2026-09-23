import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { toast } from "sonner";
import { afterEach, expect, test, vi } from "vitest";
import type { SuiteCase } from "../../lib/suiteOrder";
import ExecutionOrderModal from "./ExecutionOrderModal";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const CASES: SuiteCase[] = [
  { id: 201, title: "Valid login" },
  { id: 202, title: "Bad password" },
  { id: 203, title: "Locked out" },
];

/** Whatever locale this run uses - matches the component's own
 * `toLocaleDateString()` call exactly. */
const localDate = (iso: string) => new Date(iso).toLocaleDateString();

const FILE = (cases: Array<{ id: number; group?: string }>) => ({
  format: "tcm-run-order",
  version: 1,
  saved_by: "lead@example.com",
  saved_at: "2026-09-23T10:15:00Z",
  cases,
});

const CONFIRM = "Every tester will see this as the suggested run order for this PBI.";

type Props = ComponentProps<typeof ExecutionOrderModal>;

function mount(props: Partial<Props> = {}, backend: (cmd: string, args: unknown) => unknown = () => undefined) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return backend(cmd, args);
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const handlers = {
    onUseView: vi.fn(),
    onUseMine: vi.fn((_ids: number[]) => true),
    onClose: vi.fn(),
  };
  render(
    <QueryClientProvider client={qc}>
      <ExecutionOrderModal
        org="acme"
        project="Web"
        pbiId={42}
        cases={CASES}
        view="spec"
        file={null}
        unreadableNote={null}
        loading={false}
        myOrder={null}
        {...handlers}
        {...props}
      />
    </QueryClientProvider>,
  );
  return { calls, qc, ...handlers };
}

const startFrom = () => screen.getByRole("combobox", { name: "Start from" });
const orderList = () => screen.getByRole("list", { name: "Execution order" });
/** The list's case ids top to bottom. */
const idsOnScreen = () =>
  within(orderList())
    .getAllByRole("listitem")
    .map((li) => Number(/#(\d+)/.exec(li.textContent ?? "")?.[1]))
    .filter((n) => Number.isFinite(n));
const optionLabels = () => screen.getAllByRole("option").map((o) => o.textContent);
const pick = (name: string) => {
  fireEvent.click(startFrom());
  fireEvent.click(screen.getByRole("option", { name }));
};

/** Remembers a watched draft for PBI 42 the way the Import tab does, with
 * each case stamped with its work item id after upload. */
function watchDraft(path: string, cases: Array<{ id: number | null; order: number | null; area?: string }>) {
  const snapshot = cases.map((c, i) => ({
    title: `Case ${i}`,
    steps: [],
    tags: "",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
    update_id: c.id,
    tester_order: c.order,
    area: c.area ?? "",
  }));
  localStorage.setItem("tcm-v2-watch:acme/42", JSON.stringify([{ path, stamp: "s1", snapshot }]));
}

// ---- Start from ----

test("a found suggested order is offered first, with who saved it and when", () => {
  mount({ file: FILE([{ id: 203 }, { id: 201 }, { id: 202 }]), view: "suggested" });
  expect(screen.getByRole("heading", { name: "Execution order" })).toBeInTheDocument();
  expect(startFrom()).toHaveTextContent("Suggested run order");
  expect(idsOnScreen()).toEqual([203, 201, 202]);
  expect(screen.getByText(`Saved by lead@example.com on ${localDate("2026-09-23T10:15:00Z")}`)).toBeInTheDocument();
  fireEvent.click(startFrom());
  expect(optionLabels()).toEqual(["Suggested run order", "Spec order"]);
});

test("with no suggested order, only Spec order is offered and the note says so", () => {
  mount();
  expect(startFrom()).toHaveTextContent("Spec order");
  expect(idsOnScreen()).toEqual([201, 202, 203]);
  expect(screen.getByText("No suggested run order yet.")).toBeInTheDocument();
  fireEvent.click(startFrom());
  expect(optionLabels()).toEqual(["Spec order"]);
});

test("while the read is pending there is no note yet", () => {
  mount({ loading: true });
  expect(screen.queryByText("No suggested run order yet.")).not.toBeInTheDocument();
});

test("an unreadable file lists Suggested greyed out, with the reason", () => {
  const note = "The suggested run order could not be read: the run-order file is damaged. See Settings → Logs.";
  mount({ unreadableNote: note });
  expect(screen.getByText(note)).toBeInTheDocument();
  fireEvent.click(startFrom());
  const suggested = screen.getByRole("option", { name: "Suggested run order" });
  expect(suggested).toBeDisabled();
  fireEvent.click(suggested);
  expect(startFrom()).toHaveTextContent("Spec order");
});

test("My order is offered when this machine has one, reconciled against the cases", () => {
  mount({ myOrder: [203, 999, 201] });
  fireEvent.click(startFrom());
  expect(optionLabels()).toEqual(["Spec order", "My order"]);
  fireEvent.click(screen.getByRole("option", { name: "My order" }));
  // 999 is not one of the cases and drops out; 202 lands at the end.
  expect(idsOnScreen()).toEqual([203, 201, 202]);
});

test("an uploaded optimized draft offers its tester order", () => {
  watchDraft("C:/work/login.json", [
    { id: 201, order: 2 },
    { id: 203, order: 1 },
  ]);
  mount();
  pick("Tester order from login.json");
  expect(idsOnScreen()).toEqual([203, 201, 202]);
});

test("changing Start from replaces the list, edits included", () => {
  mount({ file: FILE([{ id: 203 }, { id: 201 }, { id: 202 }]), view: "suggested" });
  fireEvent.click(within(orderList()).getByRole("button", { name: "Move #201 up" }));
  expect(idsOnScreen()).toEqual([201, 203, 202]);
  pick("Spec order");
  expect(idsOnScreen()).toEqual([201, 202, 203]);
});

// ---- Use this order ----

test("Use this order on an unchanged Suggested run order switches the view only", () => {
  const { onUseView, onUseMine, onClose } = mount({
    file: FILE([{ id: 203 }, { id: 201 }, { id: 202 }]),
    view: "suggested",
    myOrder: [202, 201, 203],
  });
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  expect(onUseView).toHaveBeenCalledWith("suggested");
  expect(onUseMine).not.toHaveBeenCalled();
  expect(onClose).toHaveBeenCalled();
});

test("Use this order on an unchanged Spec order switches the view only", () => {
  const { onUseView, onUseMine } = mount({ file: FILE([{ id: 203 }, { id: 201 }, { id: 202 }]), view: "suggested" });
  pick("Spec order");
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  expect(onUseView).toHaveBeenCalledWith("spec");
  expect(onUseMine).not.toHaveBeenCalled();
});

test("Use this order on an unchanged My order just makes it the view", () => {
  const { onUseView, onUseMine } = mount({ myOrder: [203, 201, 202], view: "mine" });
  expect(idsOnScreen()).toEqual([203, 201, 202]);
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  expect(onUseView).toHaveBeenCalledWith("mine");
  expect(onUseMine).not.toHaveBeenCalled();
});

test("Use this order after a reorder saves the list as My order", () => {
  const { onUseView, onUseMine, onClose } = mount({
    file: FILE([{ id: 203 }, { id: 201 }, { id: 202 }]),
    view: "suggested",
  });
  fireEvent.click(within(orderList()).getByRole("button", { name: "Move #201 up" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  expect(onUseMine).toHaveBeenCalledWith([201, 203, 202]);
  expect(onUseView).not.toHaveBeenCalled();
  expect(onClose).toHaveBeenCalled();
});

test("Use this order on a tester-order start saves My order", () => {
  watchDraft("C:/work/login.json", [
    { id: 201, order: 2 },
    { id: 203, order: 1 },
  ]);
  const { onUseMine } = mount();
  pick("Tester order from login.json");
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  expect(onUseMine).toHaveBeenCalledWith([203, 201, 202]);
});

test("when My order could not be saved the modal stays open", () => {
  const onUseMine = vi.fn(() => false);
  const { onClose } = mount({ onUseMine });
  fireEvent.click(within(orderList()).getByRole("button", { name: "Move #202 up" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  expect(onUseMine).toHaveBeenCalledWith([202, 201, 203]);
  expect(onClose).not.toHaveBeenCalled();
  expect(screen.getByRole("dialog")).toBeInTheDocument();
});

test("Cancel changes nothing", () => {
  const { onUseView, onUseMine, onClose, calls } = mount();
  fireEvent.click(within(orderList()).getByRole("button", { name: "Move #202 up" }));
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(onClose).toHaveBeenCalled();
  expect(onUseView).not.toHaveBeenCalled();
  expect(onUseMine).not.toHaveBeenCalled();
  expect(calls).toEqual([]);
});

// ---- Save for everyone ----

test("Save for everyone asks first; Cancel sends nothing and goes back", () => {
  const { calls, onClose } = mount();
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  expect(screen.getByText(CONFIRM)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Use this order" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByText(CONFIRM)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Use this order" })).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
  expect(calls.some((c) => c.cmd === "save_run_order")).toBe(false);
});

test("Save sends the list, groups from the tester-order file then the saved file, and switches to Suggested", async () => {
  watchDraft("C:/work/login.json", [
    { id: 203, order: 1, area: "Auth / Lockout" },
    { id: 201, order: 2, area: "Auth" },
    { id: 202, order: 3 },
  ]);
  const NEW = { ...FILE([{ id: 203 }, { id: 201 }, { id: 202 }]), saved_by: "me@example.com" };
  const { calls, qc, onUseView, onClose } = mount(
    { file: FILE([{ id: 201 }, { id: 202, group: "Old" }, { id: 203 }]), view: "suggested" },
    (cmd) => (cmd === "save_run_order" ? NEW : undefined),
  );
  pick("Tester order from login.json");
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await vi.waitFor(() => expect(toast.success).toHaveBeenCalledWith("Suggested run order saved."));
  expect(calls.find((c) => c.cmd === "save_run_order")?.args).toEqual({
    organization: "acme",
    project: "Web",
    pbiId: 42,
    cases: [
      { id: 203, group: "Auth / Lockout" },
      { id: 201, group: "Auth" },
      { id: 202, group: "Old" },
    ],
  });
  // The shared entry Run Tests reads carries the new file at once.
  expect(qc.getQueryData(["run-order", "acme", "Web", 42])).toEqual({ state: "found", file: NEW });
  expect(onUseView).toHaveBeenCalledWith("suggested");
  expect(onClose).toHaveBeenCalled();
});

test("started from a stored order, each case keeps the group it had in the saved file", async () => {
  const { calls } = mount(
    { file: FILE([{ id: 201, group: "Auth" }, { id: 202 }, { id: 203 }]), view: "spec" },
    (cmd) => (cmd === "save_run_order" ? FILE([{ id: 201, group: "Auth" }, { id: 203 }, { id: 202 }]) : undefined),
  );
  fireEvent.click(within(orderList()).getByRole("button", { name: "Move #203 up" }));
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(calls.some((c) => c.cmd === "save_run_order")).toBe(true));
  expect(calls.find((c) => c.cmd === "save_run_order")?.args).toEqual({
    organization: "acme",
    project: "Web",
    pbiId: 42,
    cases: [{ id: 201, group: "Auth" }, { id: 203 }, { id: 202 }],
  });
});

test("a failed save says why and keeps the modal open", async () => {
  const { onClose, onUseView } = mount({}, (cmd) =>
    cmd === "save_run_order" ? Promise.reject({ kind: "Network", detail: "Could not reach Azure DevOps." }) : undefined,
  );
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith("Could not save the suggested run order: Could not reach Azure DevOps."),
  );
  expect(onClose).not.toHaveBeenCalled();
  expect(onUseView).not.toHaveBeenCalled();
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Save for everyone" })).toBeInTheDocument();
});

test("without a PBI there is no Save for everyone", () => {
  mount({ pbiId: 0 });
  expect(screen.queryByRole("button", { name: "Save for everyone" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Use this order" })).toBeInTheDocument();
});

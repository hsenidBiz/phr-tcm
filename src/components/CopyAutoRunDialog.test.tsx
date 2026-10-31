// Copying a whole environment's Auto Run data into another: the source list
// (removed environments included), the question before a non-empty target is
// replaced, and the command's refusal.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../lib/toast";
import CopyAutoRunDialog from "./CopyAutoRunDialog";

vi.mock("../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
  localStorage.clear();
});

const QA = {
  id: "env-00000002", name: "QA", start_url: "", allowed_origins: [], db_id: "qa-read",
  test_environment: false, test_prefix: "QATEST", has_default_password: false, db_auto_approve: false, password_expiry_query: "", active_session_query: "",
};
const SOURCES = [
  { key: "env-00000001", name: "Default", removed: false, days_left: null },
  { key: "removed:env-00000003-1730000000000", name: "Staging", removed: true, days_left: 12 },
];

function mount(
  choices: { target_empty: boolean; sources: typeof SOURCES },
  copy: (a: Record<string, unknown>) => unknown = () => ({ items: 5, kept_aside: false }),
) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC((cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: a });
    if (cmd === "env_copy_choices") return choices;
    if (cmd === "env_copy_autorun") return copy(a);
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <CopyAutoRunDialog target={QA} onClose={onClose} />
    </QueryClientProvider>,
  );
  return { calls, onClose };
}

test("offers removed environments as sources, labelled removed", async () => {
  mount({ target_empty: true, sources: SOURCES });
  fireEvent.click(await screen.findByRole("combobox", { name: "Copy from" }));
  expect(await screen.findByRole("option", { name: "Default" })).toBeInTheDocument();
  expect(screen.getByRole("option", { name: "Staging (removed, 12 days left)" })).toBeInTheDocument();
});

test("copies into an empty environment without asking, sending replace=false", async () => {
  const { calls, onClose } = mount({ target_empty: true, sources: SOURCES });
  const button = await screen.findByRole("button", { name: "Copy" });
  expect(screen.queryByText(/This replaces/)).not.toBeInTheDocument();
  fireEvent.click(button);
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(calls.find((c) => c.cmd === "env_copy_autorun")!.args).toEqual({
    targetId: QA.id,
    sourceKey: "env-00000001",
    replace: false,
  });
  expect(toast.success).toHaveBeenCalledWith("Copied Auto Run data into QA");
});

test("asks before replacing a non-empty environment and sends replace=true", async () => {
  const { calls, onClose } = mount({ target_empty: false, sources: SOURCES });
  expect(
    await screen.findByText(
      "This replaces QA's scripts, areas, components, quirks and API templates with Default's. QA's current ones are kept for 30 days and can be copied back from this list.",
    ),
  ).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Copy" })).not.toBeInTheDocument();
  // The question names the source that is picked.
  fireEvent.click(screen.getByRole("combobox", { name: "Copy from" }));
  fireEvent.click(await screen.findByRole("option", { name: "Staging (removed, 12 days left)" }));
  expect(screen.getByText(/with Staging's\./)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Replace" }));
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(calls.find((c) => c.cmd === "env_copy_autorun")!.args).toEqual({
    targetId: QA.id,
    sourceKey: "removed:env-00000003-1730000000000",
    replace: true,
  });
});

test("a refusal shows as an error and keeps the dialog open", async () => {
  const { onClose } = mount({ target_empty: true, sources: SOURCES }, () => {
    throw "a case is being replayed";
  });
  fireEvent.click(await screen.findByRole("button", { name: "Copy" }));
  await waitFor(() => expect(toast.error).toHaveBeenCalledWith("a case is being replayed"));
  expect(onClose).not.toHaveBeenCalled();
});

test("says so when there is nothing to copy from", async () => {
  mount({ target_empty: true, sources: [] });
  expect(await screen.findByText("There is no other environment to copy from yet.")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Copy" })).not.toBeInTheDocument();
});

test("after a needs-confirm refusal the dialog asks, and Replace is the only way to send replace=true", async () => {
  let empty = true;
  const calls: Record<string, unknown>[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "env_copy_choices") return { target_empty: empty, sources: SOURCES };
    if (cmd === "env_copy_autorun") {
      calls.push(args as Record<string, unknown>);
      if (!(args as { replace: boolean }).replace) throw "this environment already has Auto Run data - confirm to replace it";
      return { items: 1, kept_aside: true };
    }
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <CopyAutoRunDialog target={QA} onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  const copy = await screen.findByRole("button", { name: "Copy" });
  empty = false; // data arrived after the dialog opened
  fireEvent.click(copy);
  expect(await screen.findByRole("button", { name: "Replace" })).toBeInTheDocument();
  expect(calls).toHaveLength(1);
  expect(calls[0].replace).toBe(false);
  expect(toast.error).not.toHaveBeenCalled();
});

test("the replace sentence is announced politely", async () => {
  mount({ target_empty: false, sources: SOURCES });
  const sentence = await screen.findByText(/This replaces QA's/);
  expect(sentence.closest("[aria-live]")).toHaveAttribute("aria-live", "polite");
});

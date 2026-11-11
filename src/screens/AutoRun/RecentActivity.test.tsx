// Recent activity: records grouped by day with refused ones muted, a case
// link, the assistant filter, and nothing at all when nothing was recorded.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { AssistantRecord } from "../../bindings";
import { clearAutoRunFocus } from "../../lib/autoRunFocus";
import RecentActivity from "./RecentActivity";

afterEach(() => {
  clearMocks();
  clearAutoRunFocus();
  vi.clearAllMocks();
});

/** A record time `at` is UTC "YYYY-MM-DD HH:MM:SS" for a local day offset. */
function stamp(daysAgo: number, hour: number): string {
  const d = new Date();
  d.setDate(d.getDate() - daysAgo);
  d.setHours(hour, 0, 0, 0);
  return d.toISOString().slice(0, 19).replace("T", " ");
}

function rec(over: Partial<AssistantRecord> = {}): AssistantRecord {
  return { at: stamp(0, 10), label: "Assistant 1", group: "saved", action: "Saved a script", ok: true, env: "e", org: "acme", ...over };
}

function mount(records: AssistantRecord[]) {
  mockIPC((cmd) => (cmd === "auto_run_assistant_timeline" ? records : null));
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RecentActivity org="acme" />
    </QueryClientProvider>,
  );
}

test("records group by day, newest first, refused muted", async () => {
  mount([
    rec({ at: stamp(0, 10), action: "Saved a script" }),
    rec({ at: stamp(0, 9), action: "Tried an action", ok: false }),
    rec({ at: stamp(1, 15), action: "Proposed an account" }),
  ]);
  await screen.findByRole("heading", { name: "Recent activity" });
  const heads = screen.getAllByRole("heading", { level: 4 }).map((h) => h.textContent);
  expect(heads).toEqual(["Today", "Yesterday"]);
  const rows = screen.getAllByRole("listitem");
  expect(rows[1]).toHaveAttribute("data-ok", "false");
  expect(rows[0]).toHaveAttribute("data-ok", "true");
});

test("a case link opens the case", async () => {
  const { useAutoRunFocus } = await import("../../lib/autoRunFocus");
  let seen: unknown = null;
  function Probe() {
    seen = useAutoRunFocus();
    return null;
  }
  mockIPC((cmd) => (cmd === "auto_run_assistant_timeline" ? [rec({ case_id: 12 })] : null));
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RecentActivity org="acme" />
      <Probe />
    </QueryClientProvider>,
  );
  fireEvent.click(await screen.findByRole("button", { name: "Case 12" }));
  expect(seen).toEqual({ kind: "case", caseId: 12 });
});

test("the assistant filter shows only with two assistants", async () => {
  const one = mount([rec(), rec({ at: stamp(0, 8) })]);
  await screen.findByRole("heading", { name: "Recent activity" });
  expect(screen.queryByRole("button", { name: "All" })).toBeNull();
  one.unmount();
  clearMocks();

  mount([rec({ label: "Assistant 1", action: "Saved A" }), rec({ label: "Assistant 2", action: "Saved B", at: stamp(0, 8) })]);
  expect(await screen.findByRole("button", { name: "All" })).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(screen.getByRole("button", { name: "Assistant 2" }));
  expect(screen.queryByText("Saved A")).toBeNull();
  expect(screen.getByText("Saved B")).toBeInTheDocument();
});

test("nothing recorded draws nothing", async () => {
  const { container } = mount([]);
  await new Promise((r) => setTimeout(r, 30));
  expect(container.firstChild).toBeNull();
});

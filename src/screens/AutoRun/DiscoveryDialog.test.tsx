// The Discovery card: what the assistant's discovery has mapped of the live
// app, area by area - when and as whom, how much it saw, whether the map is
// stale and why, the save requests it sent, and Forget map behind a
// confirm.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { AreaView } from "../../bindings";
import DiscoveryDialog, { discoverySummary } from "./DiscoveryDialog";

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const EXPLORED = Date.UTC(2026, 9, 1, 12);

function area(over: Partial<AreaView> & { area: string }): AreaView {
  return {
    explored_at: EXPLORED,
    account: "admin",
    stale: false,
    stale_reason: null,
    pages: 0,
    elements: 0,
    writes: [],
    ...over,
  };
}

type Call = { cmd: string; args: Record<string, unknown> };

/** A map that answers like Rust's: forget takes the area out of it. */
function mount(areas: AreaView[]) {
  const map = [...areas];
  const calls: Call[] = [];
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    if (cmd === "auto_run_load_map") return { areas: map.map((a) => ({ ...a })) };
    if (cmd === "auto_run_forget_map_area") {
      const i = map.findIndex((a) => a.area === args.area);
      if (i >= 0) map.splice(i, 1);
      return null;
    }
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <DiscoveryDialog org="acme" project="Web" onClose={onClose} />
    </QueryClientProvider>,
  );
  return { calls, onClose };
}

const item = (name: string) => screen.getByRole("listitem", { name });

test("lists_areas_with_counts_and_a_stale_badge_and_its_reason", async () => {
  mount([
    area({ area: "Leave Apply", pages: 2, elements: 7 }),
    area({
      area: "Payroll",
      pages: 1,
      elements: 1,
      stale: true,
      stale_reason: "Explored more than 30 days ago",
    }),
  ]);
  expect(screen.getByRole("heading", { name: "Discovery" })).toBeInTheDocument();
  await screen.findByRole("listitem", { name: "Leave Apply" });

  const leave = item("Leave Apply");
  const date = new Date(EXPLORED).toLocaleDateString();
  expect(within(leave).getByText(`Explored ${date} as admin`)).toBeInTheDocument();
  expect(within(leave).getByText("2 pages, 7 elements")).toBeInTheDocument();
  expect(within(leave).queryByText("Stale")).not.toBeInTheDocument();

  const payroll = item("Payroll");
  expect(within(payroll).getByText("1 page, 1 element")).toBeInTheDocument();
  const badge = within(payroll).getByTitle("Explored more than 30 days ago");
  expect(badge).toHaveTextContent("Stale");
  expect(badge.className).toContain("bg-warning/15");
  expect(badge.className).toContain("text-warning");
  // The reason is heard too, not only hovered.
  expect(payroll).toHaveTextContent("Explored more than 30 days ago");
});

test("an area never explored, and an empty map, say so", async () => {
  mount([area({ area: "Reports", explored_at: null, account: null, stale: true, stale_reason: "No map yet" })]);
  const reports = await screen.findByRole("listitem", { name: "Reports" });
  expect(within(reports).getByText("Not explored yet")).toBeInTheDocument();
});

test("an empty map says nothing is explored yet", async () => {
  mount([]);
  expect(await screen.findByText(/Not explored yet/)).toBeInTheDocument();
  expect(screen.queryByRole("list", { name: "Explored areas" })).not.toBeInTheDocument();
});

test("forget_map_asks_then_calls_the_command_and_refreshes", async () => {
  const { calls } = mount([area({ area: "Leave Apply" }), area({ area: "Payroll" })]);
  await screen.findByRole("listitem", { name: "Leave Apply" });

  fireEvent.click(screen.getByRole("button", { name: "Forget map for Leave Apply" }));
  const ask = screen.getByRole("group", { name: "Forget the map for Leave Apply?" });
  expect(ask).toHaveTextContent(
    "Forget the map for Leave Apply? Scripts keep running; new saves there need the area explored again.",
  );
  expect(calls.some((c) => c.cmd === "auto_run_forget_map_area")).toBe(false);

  // Keep backs out and calls nothing.
  fireEvent.click(within(ask).getByRole("button", { name: "Keep" }));
  expect(screen.queryByRole("group", { name: "Forget the map for Leave Apply?" })).not.toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "auto_run_forget_map_area")).toBe(false);

  fireEvent.click(screen.getByRole("button", { name: "Forget map for Leave Apply" }));
  fireEvent.click(
    within(screen.getByRole("group", { name: "Forget the map for Leave Apply?" })).getByRole("button", {
      name: "Forget map",
    }),
  );
  await waitFor(() =>
    expect(screen.queryByRole("listitem", { name: "Leave Apply" })).not.toBeInTheDocument(),
  );
  expect(calls.find((c) => c.cmd === "auto_run_forget_map_area")?.args).toEqual({
    organization: "acme",
    project: "Web",
    area: "Leave Apply",
  });
  expect(screen.getByRole("listitem", { name: "Payroll" })).toBeInTheDocument();
});

test("save_requests_disclose_method_and_path", async () => {
  mount([
    area({
      area: "Leave Apply",
      writes: [
        { method: "POST", path: "/api/leave", at: EXPLORED, step: "click Save" },
        { method: "PUT", path: "/api/leave/7", at: EXPLORED, step: "click Update" },
      ],
    }),
    area({ area: "Payroll" }),
  ]);
  const leave = await screen.findByRole("listitem", { name: "Leave Apply" });
  const disclosure = within(leave).getByText("2 save requests");
  const list = within(leave).getByRole("list", { name: "Save requests in Leave Apply", hidden: true });
  expect(within(list).getAllByRole("listitem", { hidden: true }).map((li) => li.textContent)).toEqual([
    "POST /api/leave",
    "PUT /api/leave/7",
  ]);
  // Folded until asked for.
  expect(disclosure.closest("details")).not.toHaveAttribute("open");
  fireEvent.click(disclosure);
  expect(disclosure.closest("details")).toHaveAttribute("open");

  // An area that sent none has no disclosure.
  expect(within(item("Payroll")).queryByText(/save request/)).not.toBeInTheDocument();
});

test("the summary line counts explored and stale areas, or says none are explored", () => {
  expect(discoverySummary(undefined)).toBeNull();
  expect(discoverySummary({ areas: [] })).toBe("Not explored yet");
  expect(
    discoverySummary({
      areas: [
        area({ area: "A" }),
        area({ area: "B", stale: true, stale_reason: "x" }),
        area({ area: "C", explored_at: null, stale: true, stale_reason: "No map yet" }),
        area({ area: "", explored_at: null }),
      ],
    }),
  ).toBe("2 areas explored, 1 stale");
  expect(discoverySummary({ areas: [area({ area: "", explored_at: null })] })).toBe("Not explored yet");
});

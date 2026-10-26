// The Assistants card: who is connected, which browser each one has and what
// it is doing, with Stop, End discovery and Release per row. Nothing at all
// while no assistant is connected.

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import type { AgentView, AutorunAgentsChanged } from "../../bindings";

const h = vi.hoisted(() => ({
  agents: vi.fn(),
  stop: vi.fn(),
  end: vi.fn(),
  release: vi.fn(),
  emit: null as null | ((e: { payload: unknown }) => void),
  unlisten: vi.fn(),
}));

vi.mock("../../bindings", () => ({
  commands: {
    autoRunAgents: (...a: unknown[]) => h.agents(...a),
    autoRunStopAgent: (...a: unknown[]) => h.stop(...a),
    autoRunEndAgentDiscovery: (...a: unknown[]) => h.end(...a),
    autoRunReleaseAgent: (...a: unknown[]) => h.release(...a),
  },
  events: {
    autorunAgentsChanged: {
      listen: (cb: (e: { payload: unknown }) => void) => {
        h.emit = cb;
        return Promise.resolve(h.unlisten);
      },
    },
  },
}));

import AssistantsCard from "./AssistantsCard";

beforeEach(() => {
  h.agents.mockReset().mockResolvedValue([]);
  h.stop.mockReset().mockResolvedValue({ status: "ok", data: null });
  h.end.mockReset().mockResolvedValue({ status: "ok", data: null });
  h.release.mockReset().mockResolvedValue({ status: "ok", data: "Released." });
  h.unlisten.mockReset();
  h.emit = null;
});

function agent(over: Partial<AgentView> = {}): AgentView {
  return {
    label: "Assistant 1",
    client: "claude-code",
    slot: 0,
    own_browser: false,
    browser: "Edge",
    account: "hr_admin",
    activity: { kind: "replaying", case_id: 4521, step: 3, of: 7 },
    idle_minutes: 0,
    changed_for_testing: false,
    saves_blocked: false,
    can_stop: false,
    ...over,
  };
}

/** Renders the card and waits for its first load to settle. */
async function mount(agents: AgentView[]) {
  h.agents.mockResolvedValue(agents);
  const view = render(<AssistantsCard />);
  await waitFor(() => expect(h.agents).toHaveBeenCalled());
  await act(async () => {});
  return view;
}

/** Pushes an `autorun-agents-changed` event the way Rust does. */
async function push(agents: AgentView[]) {
  await waitFor(() => expect(h.emit).not.toBeNull());
  const payload: AutorunAgentsChanged = { agents };
  await act(async () => {
    h.emit!({ payload });
  });
}

test("renders nothing with no assistants", async () => {
  const { container } = await mount([]);
  expect(container).toBeEmptyDOMElement();
  expect(screen.queryByText("Assistants")).toBeNull();
});

test("names each assistant, its browser and what it does", async () => {
  await mount([agent()]);
  expect(screen.getByRole("heading", { name: "Assistants" })).toBeInTheDocument();
  expect(screen.getByText("Assistant 1 - claude-code")).toBeInTheDocument();
  expect(screen.getByText("main Auto Run browser, Edge, signed in as hr_admin")).toBeInTheDocument();
  expect(screen.getByText("replaying case 4521 (step 3 of 7)")).toBeInTheDocument();
});

test("a replay that has not reached its first step says it is starting", async () => {
  await mount([agent({ activity: { kind: "replaying", case_id: 4521, step: 0, of: 0 } })]);
  expect(screen.getByText("replaying case 4521 (starting)")).toBeInTheDocument();
  expect(screen.queryByText(/step 0 of 0/)).toBeNull();
});

test("an assistant with no client or no browser says so", async () => {
  await mount([agent({ client: null, slot: null, browser: null, account: null, activity: { kind: "idle" } })]);
  expect(screen.getByText("Assistant 1")).toBeInTheDocument();
  expect(screen.getByText("no Auto Run browser")).toBeInTheDocument();
});

test("an extra browser is called its own, and the browser and account are left out while unknown", async () => {
  await mount([agent({ own_browser: true, browser: null, account: null })]);
  expect(screen.getByText("its own browser")).toBeInTheDocument();
});

test.each([
  [{ kind: "idle" }, 0, "idle"],
  [{ kind: "idle" }, 4, "idle for 4 min"],
  [{ kind: "discovering", area: "Payroll" }, 0, "discovering Payroll"],
  [{ kind: "discovering", area: null }, 0, "discovering"],
  [{ kind: "mapping", modules: ["Payroll", "Leave"] }, 0, "mapping menus (Payroll, Leave)"],
  [{ kind: "exploring" }, 0, "exploring"],
  [{ kind: "working" }, 0, "working"],
] as const)("the activity %j after %i min reads %s", async (activity, idle, text) => {
  await mount([agent({ activity: activity as AgentView["activity"], idle_minutes: idle })]);
  expect(screen.getByText(text)).toBeInTheDocument();
});

test("the flags show only when they are set", async () => {
  const { unmount } = await mount([agent()]);
  expect(screen.queryByText("changed for testing")).toBeNull();
  expect(screen.queryByText("saves blocked")).toBeNull();
  unmount();
  await mount([agent({ changed_for_testing: true, saves_blocked: true })]);
  expect(screen.getByText("changed for testing")).toBeInTheDocument();
  expect(screen.getByText("saves blocked")).toBeInTheDocument();
});

test("Stop shows only when the assistant can be stopped, End discovery only while it explores", async () => {
  await mount([
    agent({ label: "Assistant 1", can_stop: true }),
    agent({ label: "Assistant 2", slot: 1, can_stop: false, activity: { kind: "discovering", area: null } }),
    agent({ label: "Assistant 3", slot: null, activity: { kind: "idle" } }),
  ]);
  expect(screen.getByRole("button", { name: "Stop Assistant 1" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Stop Assistant 2" })).toBeNull();
  expect(screen.queryByRole("button", { name: "End Assistant 1's discovery" })).toBeNull();
  expect(screen.getByRole("button", { name: "End Assistant 2's discovery" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Release Assistant 1's browser" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Release Assistant 2's browser" })).toBeInTheDocument();
  // No slot, so nothing to release.
  expect(screen.queryByRole("button", { name: "Release Assistant 3's browser" })).toBeNull();
});

test.each([
  [{ kind: "discovering", area: null }],
  [{ kind: "mapping", modules: ["Payroll"] }],
  [{ kind: "exploring" }],
] as const)("End discovery is offered while %j", async (activity) => {
  await mount([agent({ activity: activity as AgentView["activity"] })]);
  expect(screen.getByRole("button", { name: "End Assistant 1's discovery" })).toBeInTheDocument();
});

test("Stop, End discovery and Release send the row's slot", async () => {
  await mount([
    agent({ label: "Assistant 1", slot: 0, can_stop: true }),
    agent({ label: "Assistant 2", slot: 3, own_browser: true, activity: { kind: "exploring" } }),
  ]);
  fireEvent.click(screen.getByRole("button", { name: "Stop Assistant 1" }));
  await waitFor(() => expect(h.stop).toHaveBeenCalledWith(0));
  fireEvent.click(screen.getByRole("button", { name: "End Assistant 2's discovery" }));
  await waitFor(() => expect(h.end).toHaveBeenCalledWith(3));
  fireEvent.click(screen.getByRole("button", { name: "Release Assistant 2's browser" }));
  await waitFor(() => expect(h.release).toHaveBeenCalledWith(3));
  expect(h.stop).toHaveBeenCalledTimes(1);
});

test("a Release answer shows under its own row", async () => {
  await mount([agent({ label: "Assistant 1", slot: 0 }), agent({ label: "Assistant 2", slot: 1 })]);
  fireEvent.click(screen.getByRole("button", { name: "Release Assistant 2's browser" }));
  const rows = screen.getAllByRole("listitem");
  await waitFor(() => expect(within(rows[1]).getByText("Released.")).toBeInTheDocument());
  expect(within(rows[0]).queryByText("Released.")).toBeNull();
});

test("an error shows in the danger colour under the row", async () => {
  h.stop.mockResolvedValue({ status: "error", error: "That assistant is not running a replay." });
  await mount([agent({ can_stop: true })]);
  fireEvent.click(screen.getByRole("button", { name: "Stop Assistant 1" }));
  const msg = await screen.findByText("That assistant is not running a replay.");
  expect(msg.className).toContain("text-danger");
});

test("a command that fails outright shows an error too", async () => {
  h.release.mockRejectedValue(new Error("boom"));
  await mount([agent()]);
  fireEvent.click(screen.getByRole("button", { name: "Release Assistant 1's browser" }));
  const msg = await screen.findByRole("alert");
  expect(msg.className).toContain("text-danger");
});

test("an event replaces the rows", async () => {
  await mount([agent({ label: "Assistant 1" })]);
  expect(screen.getByText("Assistant 1 - claude-code")).toBeInTheDocument();
  await push([agent({ label: "Assistant 2", client: "codex" })]);
  expect(screen.queryByText("Assistant 1 - claude-code")).toBeNull();
  expect(screen.getByText("Assistant 2 - codex")).toBeInTheDocument();
});

test("an event with no assistants takes the card away, and one with some brings it back", async () => {
  const { container } = await mount([agent()]);
  await push([]);
  expect(container).toBeEmptyDOMElement();
  await push([agent()]);
  expect(screen.getByRole("heading", { name: "Assistants" })).toBeInTheDocument();
});

test("an event that arrives before the first load finishes is not overwritten by it", async () => {
  let release!: (v: AgentView[]) => void;
  h.agents.mockReturnValue(new Promise<AgentView[]>((r) => (release = r)));
  render(<AssistantsCard />);
  await push([agent({ label: "Assistant 9" })]);
  await act(async () => release([agent({ label: "Assistant 1" })]));
  expect(screen.getByText("Assistant 9 - claude-code")).toBeInTheDocument();
  expect(screen.queryByText("Assistant 1 - claude-code")).toBeNull();
});

test("a failed load leaves the card out rather than raising an error", async () => {
  h.agents.mockRejectedValue(new Error("no backend"));
  const { container } = render(<AssistantsCard />);
  await act(async () => {});
  expect(container).toBeEmptyDOMElement();
});

test("unmounting stops listening", async () => {
  const { unmount } = await mount([agent()]);
  unmount();
  await waitFor(() => expect(h.unlisten).toHaveBeenCalledTimes(1));
});

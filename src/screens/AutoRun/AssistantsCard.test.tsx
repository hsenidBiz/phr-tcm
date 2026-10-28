// The Assistants card: who is connected, which browser each one has and what
// it is doing, with Stop, End discovery and Release per row, a colour square
// per row, and one shared office above the rows. Nothing at all while no assistant is working.

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import type { AgentActivity, AgentView, AutorunAgentsChanged } from "../../bindings";

const h = vi.hoisted(() => ({
  agents: vi.fn(),
  stop: vi.fn(),
  end: vi.fn(),
  release: vi.fn(),
  emit: null as null | ((e: { payload: unknown }) => void),
  unlisten: vi.fn(),
  log: vi.fn(),
  /** When set, registering the listener takes until this settles. */
  slowListen: null as null | Promise<void>,
}));

vi.mock("../../bindings", () => ({
  commands: {
    autoRunAgents: (...a: unknown[]) => h.agents(...a),
    autoRunStopAgent: (...a: unknown[]) => h.stop(...a),
    autoRunEndAgentDiscovery: (...a: unknown[]) => h.end(...a),
    autoRunReleaseAgent: (...a: unknown[]) => h.release(...a),
    logUi: (...a: unknown[]) => h.log(...a),
  },
  events: {
    autorunAgentsChanged: {
      listen: (cb: (e: { payload: unknown }) => void) => {
        h.emit = cb;
        return h.slowListen ? h.slowListen.then(() => h.unlisten) : Promise.resolve(h.unlisten);
      },
    },
  },
}));

import AssistantsCard, { isWorking } from "./AssistantsCard";
import { hueFor } from "./AssistantScene";

beforeEach(() => {
  h.agents.mockReset().mockResolvedValue([]);
  h.stop.mockReset().mockResolvedValue({ status: "ok", data: null });
  h.end.mockReset().mockResolvedValue({ status: "ok", data: null });
  h.release.mockReset().mockResolvedValue({ status: "ok", data: "Released." });
  h.unlisten.mockReset();
  h.log.mockReset().mockResolvedValue(undefined);
  h.emit = null;
  h.slowListen = null;
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

test("renders nothing while every assistant is idle", async () => {
  const { container } = await mount([
    agent({ label: "Assistant 1", activity: { kind: "idle" } }),
    agent({ label: "Assistant 2", slot: 1, activity: { kind: "idle" }, idle_minutes: 4 }),
  ]);
  expect(container).toBeEmptyDOMElement();
  expect(screen.queryByRole("heading", { name: "Assistants" })).toBeNull();
});

test("shows every assistant while one is working, the idle one dozing", async () => {
  await mount([
    agent({ label: "Assistant 1", slot: 0 }),
    agent({ label: "Assistant 2", slot: 1, activity: { kind: "idle" } }),
  ]);
  expect(screen.getByText("Assistant 1 - claude-code")).toBeInTheDocument();
  expect(screen.getByText("Assistant 2 - claude-code")).toBeInTheDocument();
  const walkers = document.querySelectorAll(".assistant-scene__walker");
  expect(walkers[0]).toHaveAttribute("data-pose", "type");
  expect(walkers[1]).toHaveAttribute("data-pose", "doze");
  // The idle assistant keeps its row, so its browser can still be released.
  expect(screen.getByRole("button", { name: "Release Assistant 2's browser" })).toBeInTheDocument();
  expect(screen.getByText("1 working")).toBeInTheDocument();
});

test("the card goes away when the last working assistant goes idle", async () => {
  const { container } = await mount([agent({ label: "Assistant 1" })]);
  expect(screen.getByRole("heading", { name: "Assistants" })).toBeInTheDocument();
  await push([agent({ label: "Assistant 1", activity: { kind: "idle" } })]);
  expect(screen.queryByRole("heading", { name: "Assistants" })).toBeNull();
  expect(container).toBeEmptyDOMElement();
});

test("the header counts the assistants that are working", async () => {
  await mount([
    agent({ label: "Assistant 1", slot: 0 }),
    agent({ label: "Assistant 2", slot: 1, activity: { kind: "exploring" } }),
    agent({ label: "Assistant 3", slot: 2, activity: { kind: "idle" } }),
  ]);
  expect(screen.getByText("2 working")).toBeInTheDocument();
});

test("isWorking is true for every activity except idle", () => {
  expect(isWorking({ kind: "idle" })).toBe(false);
  const busy: AgentActivity[] = [
    { kind: "discovering", area: null },
    { kind: "mapping", modules: [] },
    { kind: "exploring" },
    { kind: "replaying", case_id: 1, step: 0, of: 0 },
    { kind: "working" },
  ];
  for (const a of busy) {
    expect(isWorking(a)).toBe(true);
  }
});

test("a replay shows its progress, full at its last step", async () => {
  const { unmount } = await mount([agent({ activity: { kind: "replaying", case_id: 4521, step: 3, of: 7 } })]);
  const bar = document.querySelector<HTMLElement>("[data-progress]");
  expect(bar).not.toBeNull();
  expect(bar).toHaveAttribute("aria-hidden", "true");
  expect((bar!.firstElementChild as HTMLElement).style.width).toBe("43%");
  unmount();

  await mount([agent({ activity: { kind: "replaying", case_id: 4521, step: 7, of: 7 } })]);
  const full = document.querySelector<HTMLElement>("[data-progress]");
  expect((full!.firstElementChild as HTMLElement).style.width).toBe("100%");
});

test("a replay at step 0 of 0 shows no bar, and no NaN width", async () => {
  await mount([agent({ activity: { kind: "replaying", case_id: 4521, step: 0, of: 0 } })]);
  expect(document.querySelector("[data-progress]")).toBeNull();
  expect(document.body.innerHTML).not.toContain("NaN");
});

test("only a replay has a progress bar", async () => {
  await mount([agent({ activity: { kind: "exploring" } })]);
  expect(document.querySelector("[data-progress]")).toBeNull();
});

test("an assistant with no Auto Run browser has no row and no place in the office", async () => {
  await mount([
    agent({ label: "Assistant 1", slot: null, browser: null, account: null, activity: { kind: "idle" } }),
    agent({ label: "Assistant 2", slot: 1, activity: { kind: "exploring" } }),
  ]);
  expect(screen.queryByText(/^Assistant 1/, { ignore: "script, style" })).toBeNull();
  expect(screen.queryByText("No Auto Run browser")).toBeNull();
  expect(screen.getByText("1 working")).toBeInTheDocument();
  const walkers = document.querySelectorAll(".assistant-scene__walker");
  expect(walkers).toHaveLength(1);
  expect(walkers[0]).toHaveAttribute("data-slot", "1");
});

test("the card stays away while only assistants with no Auto Run browser are working", async () => {
  await mount([agent({ slot: null, browser: null, account: null, activity: { kind: "working" } })]);
  expect(screen.queryByRole("heading", { name: "Assistants" })).toBeNull();
  expect(document.querySelector(".assistant-scene")).toBeNull();
});

test("names each assistant, its browser and what it does", async () => {
  await mount([agent()]);
  expect(screen.getByRole("heading", { name: "Assistants" })).toBeInTheDocument();
  expect(screen.getByText("Assistant 1 - claude-code")).toBeInTheDocument();
  expect(screen.getByText("Main Auto Run browser, Edge, signed in as hr_admin")).toBeInTheDocument();
  expect(screen.getByText("Replaying case 4521 (step 3 of 7)")).toBeInTheDocument();
});

test("a replay that has not reached its first step says it is starting", async () => {
  await mount([agent({ activity: { kind: "replaying", case_id: 4521, step: 0, of: 0 } })]);
  expect(screen.getByText("Replaying case 4521 (starting)")).toBeInTheDocument();
  expect(screen.queryByText(/step 0 of 0/)).toBeNull();
});

test("an assistant with no client is named by its label alone", async () => {
  await mount([agent({ client: null })]);
  // The office's name tag repeats the label, so look only at the row.
  expect(screen.getByText("Assistant 1", { ignore: ".assistant-scene *, script, style" })).toBeInTheDocument();
});

test("an extra browser is called its own, and the browser and account are left out while unknown", async () => {
  await mount([agent({ own_browser: true, browser: null, account: null })]);
  expect(screen.getByText("Its own browser")).toBeInTheDocument();
});

test("one assistant in the main browser and one in its own browser read apart, each with its account", async () => {
  await mount([
    agent({ label: "Assistant 1", slot: 0, account: "hr_admin" }),
    agent({
      label: "Assistant 2",
      slot: 1,
      own_browser: true,
      browser: "Edge",
      account: "portal.reviewer",
      activity: { kind: "discovering", area: "Payroll" },
    }),
  ]);
  expect(screen.getByText("Main Auto Run browser, Edge, signed in as hr_admin")).toBeInTheDocument();
  expect(screen.getByText("Its own browser, Edge, signed in as portal.reviewer")).toBeInTheDocument();
  expect(screen.getByText("Discovering Payroll")).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: /^Release Assistant \d's browser$/ })).toHaveLength(2);
});

test.each([
  [{ kind: "idle" }, 0, "Idle"],
  [{ kind: "idle" }, 4, "Idle for 4 min"],
  [{ kind: "discovering", area: "Payroll" }, 0, "Discovering Payroll"],
  [{ kind: "discovering", area: null }, 0, "Discovering"],
  [{ kind: "mapping", modules: ["Payroll", "Leave"] }, 0, "Mapping menus (Payroll, Leave)"],
  [{ kind: "exploring" }, 0, "Exploring"],
  [{ kind: "working" }, 0, "Working"],
] as const)("the activity %j after %i min reads %s", async (activity, idle, text) => {
  // The card only shows while someone works, so an idle row needs a working
  // assistant beside it.
  const beside =
    activity.kind === "idle" ? [agent({ label: "Assistant 2", slot: 1, activity: { kind: "exploring" } })] : [];
  await mount([agent({ activity: activity as AgentView["activity"], idle_minutes: idle }), ...beside]);
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
  expect(screen.queryByRole("button", { name: "End discovery for Assistant 1" })).toBeNull();
  expect(screen.getByRole("button", { name: "End discovery for Assistant 2" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Release Assistant 1's browser" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Release Assistant 2's browser" })).toBeInTheDocument();
  // No slot, so no row and nothing to release.
  expect(screen.queryByRole("button", { name: "Release Assistant 3's browser" })).toBeNull();
});

test.each([
  [{ kind: "discovering", area: null }],
  [{ kind: "mapping", modules: ["Payroll"] }],
  [{ kind: "exploring" }],
] as const)("End discovery is offered while %j", async (activity) => {
  await mount([agent({ activity: activity as AgentView["activity"] })]);
  expect(screen.getByRole("button", { name: "End discovery for Assistant 1" })).toBeInTheDocument();
});

test("Stop, End discovery and Release send the row's slot", async () => {
  await mount([
    agent({ label: "Assistant 1", slot: 0, can_stop: true }),
    agent({ label: "Assistant 2", slot: 3, own_browser: true, activity: { kind: "exploring" } }),
  ]);
  fireEvent.click(screen.getByRole("button", { name: "Stop Assistant 1" }));
  await waitFor(() => expect(h.stop).toHaveBeenCalledWith(0));
  fireEvent.click(screen.getByRole("button", { name: "End discovery for Assistant 2" }));
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

test.each([
  ["stop", "Stop Assistant 1", "Could not stop Assistant 1. Try again, or see Settings → Logs."],
  ["end", "End discovery for Assistant 1", "Could not end Assistant 1's discovery. Try again, or see Settings → Logs."],
  ["release", "Release Assistant 1's browser", "Could not release Assistant 1's browser. Try again, or see Settings → Logs."],
] as const)("when %s throws, the row says which action failed and the cause is logged", async (which, button, sentence) => {
  h[which].mockRejectedValue(new Error("error sending request for url (https://example.test/x)"));
  await mount([agent({ can_stop: true, activity: { kind: "exploring" } })]);
  fireEvent.click(screen.getByRole("button", { name: button }));
  const msg = await screen.findByRole("alert");
  expect(msg).toHaveTextContent(sentence);
  expect(msg.textContent).toBe(sentence);
  expect(msg.className).toContain("text-danger");
  // The cause goes to the app log, never to the person.
  expect(h.log).toHaveBeenCalledTimes(1);
  expect(String(h.log.mock.calls[0][0])).toContain("example.test");
});

test("a row's buttons wait while its own command is on its way", async () => {
  let done!: (r: unknown) => void;
  h.release.mockReturnValue(new Promise((r) => (done = r)));
  await mount([
    agent({ label: "Assistant 1", slot: 0, can_stop: true, activity: { kind: "exploring" } }),
    agent({ label: "Assistant 2", slot: 1, can_stop: true }),
  ]);
  fireEvent.click(screen.getByRole("button", { name: "Release Assistant 1's browser" }));
  const one = ["Stop Assistant 1", "End discovery for Assistant 1", "Release Assistant 1's browser"];
  const two = ["Stop Assistant 2", "Release Assistant 2's browser"];
  await waitFor(() => expect(screen.getByRole("button", { name: one[0] })).toBeDisabled());
  for (const n of one) expect(screen.getByRole("button", { name: n })).toBeDisabled();
  for (const n of two) expect(screen.getByRole("button", { name: n })).toBeEnabled();
  await act(async () => done({ status: "ok", data: "Released." }));
  for (const n of one) expect(screen.getByRole("button", { name: n })).toBeEnabled();
});

test("each row keeps one status element, and only its text changes", async () => {
  await mount([agent({ label: "Assistant 1", slot: 0 }), agent({ label: "Assistant 2", slot: 1 })]);
  const rows = screen.getAllByRole("listitem");
  const before = within(rows[0]).getByRole("status");
  expect(before).toHaveAttribute("aria-live", "polite");
  expect(before).toBeEmptyDOMElement();
  fireEvent.click(screen.getByRole("button", { name: "Release Assistant 1's browser" }));
  await waitFor(() => expect(before).toHaveTextContent("Released."));
  expect(within(screen.getAllByRole("listitem")[0]).getByRole("status")).toBe(before);
  expect(within(rows[1]).getByRole("status")).toBeEmptyDOMElement();
});

test("a new Assistant 1 does not inherit the old one's answer or error", async () => {
  h.release.mockResolvedValue({ status: "error", error: "The browser is busy." });
  await mount([agent({ label: "Assistant 1" })]);
  fireEvent.click(screen.getByRole("button", { name: "Release Assistant 1's browser" }));
  await screen.findByText("The browser is busy.");
  await push([]);
  await push([agent({ label: "Assistant 1" })]);
  expect(screen.queryByText("The browser is busy.")).toBeNull();
});

test("an assistant that is still there keeps its answer when the list refreshes", async () => {
  await mount([agent({ label: "Assistant 1" })]);
  fireEvent.click(screen.getByRole("button", { name: "Release Assistant 1's browser" }));
  await screen.findByText("Released.");
  await push([agent({ label: "Assistant 1", idle_minutes: 1 })]);
  expect(screen.getByText("Released.")).toBeInTheDocument();
});

test("an event fired while the first load is still going is not lost", async () => {
  let release!: (v: AgentView[]) => void;
  h.agents.mockReturnValue(new Promise<AgentView[]>((r) => (release = r)));
  render(<AssistantsCard />);
  // The listener is already registered before the load has answered.
  await waitFor(() => expect(h.emit).not.toBeNull());
  expect(h.agents).toHaveBeenCalled();
  await push([agent({ label: "Assistant 7" })]);
  await act(async () => release([agent({ label: "Assistant 1" })]));
  expect(screen.getByText("Assistant 7 - claude-code")).toBeInTheDocument();
});

test("the load waits for the listener to be registered", async () => {
  let registered!: () => void;
  const order: string[] = [];
  h.agents.mockImplementation(() => {
    order.push("load");
    return Promise.resolve([]);
  });
  // A listener whose registration is slow.
  h.slowListen = new Promise<void>((r) => (registered = r)).then(() => {
    order.push("listening");
  });
  render(<AssistantsCard />);
  await act(async () => {});
  expect(order).toEqual([]);
  await act(async () => registered());
  expect(order).toEqual(["listening", "load"]);
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

test("the office renders once, above a plain list of rows, each with a square in its character's colour", async () => {
  await mount([agent({ label: "Assistant 1", slot: 0 }), agent({ label: "Assistant 2", slot: 1 })]);
  expect(document.querySelectorAll(".assistant-scene")).toHaveLength(1);
  const list = await screen.findByRole("list");
  expect(list).not.toHaveClass("grid");
  expect(list.querySelector(".assistant-scene")).toBeNull();
  const office = document.querySelector(".assistant-scene")!;
  expect(office.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  const squares = list.querySelectorAll<HTMLElement>("[data-hue]");
  expect(squares).toHaveLength(2);
  expect(squares[0].style.background).toBe(hueFor(0));
  expect(squares[1].style.background).toBe(hueFor(1));
});

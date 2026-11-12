// The activity strip: nothing while idle, three lines then a count, what a
// line says, Stop only where it works, a trial with no Stop, and where the
// strip sits in the layout.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, test, vi } from "vitest";
import type { AgentView } from "../bindings";
import ActivityStrip from "./ActivityStrip";

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

function agent(over: Partial<AgentView> = {}): AgentView {
  return {
    label: "Assistant 1",
    client: "claude-code",
    slot: 0,
    own_browser: false,
    browser: null,
    account: null,
    activity: { kind: "exploring" },
    idle_minutes: 0,
    changed_for_testing: false,
    saves_blocked: false,
    can_stop: false,
    ...over,
  };
}

function mount(agents: AgentView[], trial: { environment: string } | null = null, stop = vi.fn()) {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_agents") return agents;
    if (cmd === "auto_run_sign_in_trial") return trial;
    if (cmd === "auto_run_stop_agent") {
      stop(args);
      return null;
    }
    return null;
  });
  return { ...render(<ActivityStrip onOpen={() => {}} />), stop };
}

test("all idle draws nothing", async () => {
  const { container } = mount([agent({ activity: { kind: "idle" } })]);
  await new Promise((r) => setTimeout(r, 30));
  expect(container.firstChild).toBeNull();
});

test("three lines, then and N more", async () => {
  mount([1, 2, 3, 4].map((n) => agent({ label: `Assistant ${n}`, slot: n })));
  expect(await screen.findByText("and 1 more")).toBeInTheDocument();
  expect(screen.getAllByRole("listitem")).toHaveLength(4);
});

test("a line says label, doing, browser and account", async () => {
  mount([
    agent({ activity: { kind: "replaying", case_id: 5, step: 2, of: 9 }, browser: "Edge", account: "hr.sup" }),
  ]);
  expect(await screen.findByText("Assistant 1 · Replaying case 5 (step 2 of 9) · Edge · hr.sup")).toBeInTheDocument();
});

test("Stop only when the assistant can be stopped", async () => {
  const stop = vi.fn();
  const first = mount([agent({ can_stop: false })], null, stop);
  await screen.findByText(/Assistant 1/);
  expect(screen.queryByRole("button", { name: "Stop Assistant 1" })).toBeNull();
  first.unmount();

  mount([agent({ can_stop: true, slot: 2 })], null, stop);
  fireEvent.click(await screen.findByRole("button", { name: "Stop Assistant 1" }));
  await waitFor(() => expect(stop).toHaveBeenCalledWith({ slot: 2 }));
});

test("a second Stop click while the first is pending calls once, and re-enables after", async () => {
  let release: (v: null) => void = () => {};
  const stop = vi.fn();
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_agents") return [agent({ can_stop: true, slot: 2 })];
    if (cmd === "auto_run_sign_in_trial") return null;
    if (cmd === "auto_run_stop_agent") {
      stop(args);
      return new Promise<null>((res) => (release = res));
    }
    return null;
  });
  render(<ActivityStrip onOpen={() => {}} />);
  const btn = await screen.findByRole("button", { name: "Stop Assistant 1" });
  fireEvent.click(btn);
  await waitFor(() => expect(btn).toBeDisabled());
  fireEvent.click(btn);
  expect(stop).toHaveBeenCalledTimes(1);
  release(null);
  await waitFor(() => expect(btn).toBeEnabled());
  expect(stop).toHaveBeenCalledTimes(1);
});

test("a sign-in trial shows with no Stop", async () => {
  mount([agent({ activity: { kind: "idle" } })], { environment: "Staging" });
  expect(await screen.findByText("Trying a sign-in · Staging")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Stop/ })).toBeNull();
});

test("the strip is in the layout flow above main", () => {
  const src = join(dirname(fileURLToPath(import.meta.url)), "..");
  const source = (f: string) => readFileSync(join(src, f), "utf-8");
  const app = source("App.tsx");
  expect(app.indexOf("<ActivityStrip")).toBeGreaterThan(-1);
  expect(app.indexOf("<ActivityStrip")).toBeLessThan(app.indexOf("<main"));
  const strip = source("components/ActivityStrip.tsx");
  expect(strip).not.toMatch(/\b(fixed|absolute|sticky)\b|\bz-\d/);
});

test("the interrupted-upload check is keyed by account and organisation", () => {
  // A different account signing in must list its own journals, so the
  // once-per-session key and the effect's dependencies both carry the account.
  const src = join(dirname(fileURLToPath(import.meta.url)), "..");
  const app = readFileSync(join(src, "App.tsx"), "utf-8");
  expect(app).toContain("const checkKey = `${journalAccount}|${org}`;");
  expect(app).toContain("}, [signedIn, org, tourOpen, journalAccount]);");
});

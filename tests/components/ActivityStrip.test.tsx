// The activity strip: nothing while idle, one short line, the Open Auto Test
// button, a trial with no Stop, and where the strip sits in the layout.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, test, vi } from "vitest";
import type { AgentView } from "../../src/bindings";
import ActivityStrip from "../../src/components/ActivityStrip";

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
  return { ...render(<ActivityStrip onAutoTest onOpen={() => {}} />), stop };
}

test("all idle draws nothing", async () => {
  const { container } = mount([agent({ activity: { kind: "idle" } })]);
  await new Promise((r) => setTimeout(r, 30));
  expect(container.firstChild).toBeNull();
});

// The strip is one short line now: the old per-assistant lines, their
// label/browser/account text and the per-assistant Stop buttons moved to the
// title (detail) and to Auto Test's Assistants card (Stop).
const discovering = (n: number, area: string | null = "Performance & Evaluation / Admin / Strategic Objectives") =>
  agent({ label: `Assistant ${n}`, slot: n, activity: { kind: "discovering", area } });

test("one discovering assistant names the last part of the screen path", async () => {
  mount([discovering(1)]);
  expect(await screen.findByText("Discovery is running · Strategic Objectives")).toBeInTheDocument();
});

test("two discovering assistants add (+1 more)", async () => {
  mount([discovering(1), discovering(2)]);
  expect(await screen.findByText("Discovery is running · Strategic Objectives (+1 more)")).toBeInTheDocument();
});

test("non-discovering assistants are counted", async () => {
  const two = mount([agent({ slot: 1 }), agent({ label: "Assistant 2", slot: 2 })]);
  expect(await screen.findByText("2 assistants are working")).toBeInTheDocument();
  two.unmount();
  mount([agent()]);
  expect(await screen.findByText("An assistant is working")).toBeInTheDocument();
});

test("the Open Auto Test button is absent on Auto Test and calls onOpen elsewhere", async () => {
  const open = vi.fn();
  mockIPC((cmd) => (cmd === "auto_run_agents" ? [agent()] : null));
  const on = render(<ActivityStrip onAutoTest={false} onOpen={open} />);
  await screen.findByText("An assistant is working");
  expect(screen.queryByRole("button", { name: "Open Auto Test" })).toBeNull();
  on.unmount();
  render(<ActivityStrip onAutoTest onOpen={open} />);
  fireEvent.click(await screen.findByRole("button", { name: "Open Auto Test" }));
  expect(open).toHaveBeenCalledTimes(1);
});

test("the line is plain text everywhere; the Open Auto Test button is the one way in", async () => {
  mockIPC((cmd) => (cmd === "auto_run_agents" ? [agent()] : null));
  render(<ActivityStrip onAutoTest onOpen={() => {}} />);
  const line = await screen.findByText("An assistant is working");
  expect(line.tagName).toBe("P");
  expect(screen.getAllByRole("button")).toHaveLength(1);
});

test("the area comes from the first discovering assistant that has one", async () => {
  mount([discovering(1, null), discovering(2, "Admin / Leave Types")]);
  expect(await screen.findByText("Discovery is running · Leave Types (+1 more)")).toBeInTheDocument();
});

test("a sign-in trial next to working assistants prefixes the line", async () => {
  mount([discovering(1)], { environment: "Staging" });
  expect(
    await screen.findByText("Checking a sign-in · Discovery is running · Strategic Objectives"),
  ).toBeInTheDocument();
});

test("no per-assistant Stop in the strip", async () => {
  mount([agent({ can_stop: true, slot: 2 })]);
  await screen.findByText("An assistant is working");
  expect(screen.queryByRole("button", { name: /^Stop/ })).toBeNull();
});

test("the title holds the full per-assistant detail", async () => {
  mount([
    agent({ activity: { kind: "replaying", case_id: 5, step: 2, of: 9 }, browser: "Edge", account: "hr.sup" }),
  ]);
  const line = await screen.findByText("An assistant is working");
  expect(line).toHaveAttribute("title", "Assistant 1 · Replaying case 5 (step 2 of 9) · Edge · hr.sup");
});

test("a sign-in trial shows its own line with no Stop", async () => {
  mount([agent({ activity: { kind: "idle" } })], { environment: "Staging" });
  expect(await screen.findByText("Trying a sign-in · Staging")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Stop/ })).toBeNull();
});

test("the strip is in the layout flow above main", () => {
  const src = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "src");
  const source = (f: string) => readFileSync(join(src, f), "utf-8");
  const app = source("App.tsx");
  expect(app.indexOf("<ActivityStrip")).toBeGreaterThan(-1);
  expect(app.indexOf("<ActivityStrip")).toBeLessThan(app.indexOf("<main"));
  const strip = source("components/ActivityStrip.tsx");
  expect(strip).not.toMatch(/\b(fixed|absolute|sticky)\b|\bz-\d/);
});

test("Work Manager counts as not being on Auto Test, and the button leaves it", () => {
  // `section` can still read "autorun" while Work Manager is showing, so the
  // flag is built from workMode too; goToSection exits Work Manager.
  const src = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "src");
  const app = readFileSync(join(src, "App.tsx"), "utf-8");
  expect(app).toContain('<ActivityStrip onAutoTest={workMode || section !== "autorun"} onOpen={() => goToSection("autorun")} />');
  const nav = readFileSync(join(src, "app/useAppNavigation.ts"), "utf-8");
  expect(nav).toMatch(/const goToSection[\s\S]*?setWorkMode\(false\)/);
});

test("the interrupted-upload check is keyed by account and organisation", () => {
  // A different account signing in must list its own journals, so the
  // once-per-session key and the effect's dependencies both carry the account.
  const src = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "src");
  const app = readFileSync(join(src, "app/useBackendEvents.ts"), "utf-8");
  expect(app).toContain("const checkKey = `${journalAccount}|${org}`;");
  expect(app).toContain("}, [signedIn, org, tourOpen, journalAccount]);");
});

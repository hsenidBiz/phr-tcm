// The sample Assistants card shows what several assistants at once look
// like: one in the main Auto Run browser and one in a browser of its own,
// each signed in as a different account.

import { afterEach, beforeEach, expect, test, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  once: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(() => Promise.resolve()),
}));

beforeEach(() => {
  vi.resetModules();
  localStorage.setItem("tcm-v2-dev-demo", "on");
});
afterEach(() => localStorage.clear());

async function sampleAgents() {
  const m = await import("./demo");
  m.maybeEnableDemoMode();
  const { commands } = await import("../bindings");
  return (commands as unknown as { autoRunAgents: () => Promise<import("../bindings").AgentView[]> }).autoRunAgents();
}

test("the sample assistants work in two browsers, the second one its own", async () => {
  const agents = await sampleAgents();
  expect(agents.map((a) => a.label)).toEqual(["Assistant 1", "Assistant 2"]);
  const [main, extra] = agents;
  expect(main.slot).toBe(0);
  expect(main.own_browser).toBe(false);
  expect(extra.slot).toBe(1);
  expect(extra.own_browser).toBe(true);
  expect(extra.browser).toBe("Edge");
  expect(extra.activity.kind).toBe("discovering");
});

test("each sample assistant is signed in as an account of its own", async () => {
  const agents = await sampleAgents();
  const accounts = agents.map((a) => a.account);
  expect(accounts.every((a) => typeof a === "string" && a.length > 0)).toBe(true);
  expect(new Set(accounts).size).toBe(accounts.length);
});

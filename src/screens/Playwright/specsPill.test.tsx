// The specs pill beside the Playwright title: whether the clone's spec files
// are found and read, and kept current when they change. The screen
// (Playwright) does the watching; the pill (SpecsStatusPill) is drawn by App
// outside it, so both are mounted here.
//
// Only setInterval is faked: the 60 s check is stepped by hand while
// Testing Library and React Query keep their real timers.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { RepoStructure } from "../../bindings";
import SpecsStatusPill from "../../components/SpecsStatusPill";
import { SPECS_CHECK_MS, SUITES_MISSING, getSpecsPill } from "../../lib/pwSpecs";
import Playwright from "./index";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
});

afterEach(() => {
  vi.useRealTimers();
  clearMocks();
  vi.clearAllMocks();
});

const STRUCTURE: RepoStructure = {
  features: [
    { path: ["sl", "admin", "rating"], navigation_key: "admin/rating", spec_count: 4, case_count: 1, case_ids: [1] },
  ],
  generated_ids: [],
  user_keys: [],
  navigation_keys: [],
  health: [],
};

type Walk = { count: number; fingerprint: string } | { fails: string };

/** A fake clone: `walk` is what the spec look answers, `structure` what the
 * structure command does (`fails` makes it throw). */
function fake(initial: Walk, clone = "C:/repo") {
  const calls: string[] = [];
  const state = {
    walk: initial,
    structureFails: false,
    /** Holds the structure read until released, so the amber state can be seen. */
    gate: null as null | Promise<void>,
  };
  mockIPC(async (cmd) => {
    calls.push(cmd);
    if (cmd === "get_app_settings") return { playwright_clone: clone };
    if (cmd === "pw_repo_specs_status") {
      if ("fails" in state.walk) throw state.walk.fails;
      return state.walk;
    }
    if (cmd === "pw_repo_structure") {
      if (state.gate) await state.gate;
      if (state.structureFails) throw "The clone could not be read.";
      return STRUCTURE;
    }
    if (cmd === "pw_reports" || cmd === "pw_users_list" || cmd === "pw_sync_states") return [];
    return null;
  });
  return { calls, state, n: (cmd: string) => calls.filter((c) => c === cmd).length };
}

function Both() {
  return (
    <>
      <SpecsStatusPill />
      <Playwright org="acme" project="Web" />
    </>
  );
}

function mount(qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })) {
  const view = render(
    <QueryClientProvider client={qc}>
      <Both />
    </QueryClientProvider>,
  );
  return { qc, ...view };
}

/** The minute passes. */
async function tick() {
  await act(async () => {
    vi.advanceTimersByTime(SPECS_CHECK_MS);
  });
}

const pill = () => screen.getByRole("status", { name: /^Playwright specs:/ });

test("specs found and read: green, with the count, and why on hover and to a screen reader", async () => {
  fake({ count: 4, fingerprint: "a" });
  mount();
  const p = await screen.findByRole("status", { name: /^Playwright specs: 4 specs ready\./ });
  expect(p).toHaveTextContent("4 specs ready");
  expect(p.className).toContain("text-success");
  expect(p.querySelector(".status-glow")).toBeTruthy();
  expect(p).toHaveAttribute("title", expect.stringContaining("4 specs found"));
  expect(p.getAttribute("aria-label")).toContain(p.getAttribute("title"));
});

test("one spec says spec, not specs", async () => {
  fake({ count: 1, fingerprint: "a" });
  mount();
  expect(await screen.findByText("1 spec ready")).toBeInTheDocument();
});

test("a suites folder with no specs is red: No specs found", async () => {
  fake({ count: 0, fingerprint: "a" });
  mount();
  const p = await screen.findByRole("status", { name: /^Playwright specs: No specs found\./ });
  expect(p.className).toContain("text-danger");
  expect(p).toHaveAttribute("title", expect.stringContaining("no spec files"));
});

test("a missing suites folder is red: No specs found, with the sentence", async () => {
  fake({ fails: SUITES_MISSING });
  mount();
  const p = await screen.findByText("No specs found");
  expect(p.closest("[role=status]")).toHaveAttribute("title", SUITES_MISSING);
  expect(p.closest("[role=status]")!.className).toContain("text-danger");
});

test("a walk that failed is red: Specs could not be read, with the sentence", async () => {
  fake({ fails: "The spec files could not be read. Press Refresh." });
  mount();
  const p = (await screen.findByText("Specs could not be read")).closest("[role=status]")!;
  expect(p.className).toContain("text-danger");
  expect(p).toHaveAttribute("title", "The spec files could not be read. Press Refresh.");
  expect(p.getAttribute("aria-label")).toContain("The spec files could not be read. Press Refresh.");
});

test("with no clone set there is no pill and no look at the specs", async () => {
  const f = fake({ count: 4, fingerprint: "a" }, "");
  mount();
  await screen.findByText("Choose your clone of the team's PHR-PLAYWRIGHT-AUTOMATION repo.");
  await tick();
  expect(screen.queryByRole("status", { name: /^Playwright specs:/ })).not.toBeInTheDocument();
  expect(f.n("pw_repo_specs_status")).toBe(0);
});

test("the first look reads nothing again: what the tab just loaded is current", async () => {
  const f = fake({ count: 4, fingerprint: "a" });
  mount();
  await screen.findByText("4 specs ready");
  expect(f.n("pw_repo_structure")).toBe(1);
});

test("a changed fingerprint shows amber while the clone is read again, once, then green", async () => {
  const f = fake({ count: 4, fingerprint: "a" });
  mount();
  await screen.findByText("4 specs ready");
  expect(f.n("pw_repo_structure")).toBe(1);

  let release!: () => void;
  f.state.gate = new Promise<void>((r) => (release = r));
  f.state.walk = { count: 5, fingerprint: "b" };
  await tick();
  const amber = await screen.findByText("Updating specs…");
  expect(amber.closest("[role=status]")!.className).toContain("text-warning");
  await waitFor(() => expect(f.n("pw_repo_structure")).toBe(2));

  // Another minute passes while it is still going: nothing starts twice.
  await tick();
  expect(f.n("pw_repo_structure")).toBe(2);

  await act(async () => release());
  expect(await screen.findByText("5 specs ready")).toBeInTheDocument();
  expect(f.n("pw_repo_structure")).toBe(2);

  // Settled on the new fingerprint: more minutes read nothing.
  f.state.gate = null;
  await tick();
  await tick();
  expect(f.n("pw_repo_structure")).toBe(2);
});

test("an unchanged fingerprint reads nothing, however many minutes pass", async () => {
  const f = fake({ count: 4, fingerprint: "a" });
  mount();
  await screen.findByText("4 specs ready");
  const looks = f.n("pw_repo_specs_status");
  await tick();
  await tick();
  await tick();
  await waitFor(() => expect(f.n("pw_repo_specs_status")).toBe(looks + 3));
  expect(f.n("pw_repo_structure")).toBe(1);
  expect(screen.getByText("4 specs ready")).toBeInTheDocument();
});

test("a window that regains focus looks at the specs", async () => {
  const f = fake({ count: 4, fingerprint: "a" });
  mount();
  await screen.findByText("4 specs ready");
  const looks = f.n("pw_repo_specs_status");
  f.state.walk = { count: 4, fingerprint: "b" };
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  await waitFor(() => expect(f.n("pw_repo_specs_status")).toBe(looks + 1));
  await waitFor(() => expect(f.n("pw_repo_structure")).toBe(2));
});

test("the minute check only runs while the tab is shown", async () => {
  const f = fake({ count: 4, fingerprint: "a" });
  const { unmount } = mount();
  await screen.findByText("4 specs ready");
  await tick();
  const looks = f.n("pw_repo_specs_status");

  unmount();
  await tick();
  await tick();
  window.dispatchEvent(new Event("focus"));
  expect(f.n("pw_repo_specs_status")).toBe(looks);
  expect(getSpecsPill()).toBeNull();
});

test("coming back to the tab after the specs changed reads the clone again", async () => {
  const f = fake({ count: 4, fingerprint: "a" });
  const first = mount();
  await screen.findByText("4 specs ready");
  first.unmount();

  f.state.walk = { count: 6, fingerprint: "b" };
  mount(first.qc);
  expect(await screen.findByText("6 specs ready")).toBeInTheDocument();
  await waitFor(() => expect(f.n("pw_repo_structure")).toBe(2));
});

test("a read that fails is red, is not tried again until the specs change, and Refresh tries it", async () => {
  const f = fake({ count: 4, fingerprint: "a" });
  mount();
  await screen.findByText("4 specs ready");

  f.state.structureFails = true;
  f.state.walk = { count: 4, fingerprint: "b" };
  await tick();
  const red = (await screen.findByText("Specs could not be read")).closest("[role=status]")!;
  expect(red.className).toContain("text-danger");
  expect(red).toHaveAttribute("title", expect.stringContaining("The clone could not be read."));
  expect(red).toHaveAttribute("title", expect.stringContaining("Press Refresh to try again."));
  expect(f.n("pw_repo_structure")).toBe(2);

  // The same fingerprint again: the failing read is not repeated on a timer.
  await tick();
  await tick();
  expect(f.n("pw_repo_structure")).toBe(2);

  // The specs change again: it is tried again.
  f.state.walk = { count: 4, fingerprint: "c" };
  await tick();
  await waitFor(() => expect(f.n("pw_repo_structure")).toBe(3));

  // Refresh is the person's retry.
  f.state.structureFails = false;
  await act(async () => {
    screen.getByRole("button", { name: "Refresh everything" }).click();
  });
  expect(await screen.findByText("4 specs ready")).toBeInTheDocument();
  expect(f.n("pw_repo_structure")).toBe(4);
});

test("Refresh reads the clone again even when the specs look the same", async () => {
  const f = fake({ count: 4, fingerprint: "a" });
  mount();
  await screen.findByText("4 specs ready");
  await act(async () => {
    screen.getByRole("button", { name: "Refresh everything" }).click();
  });
  await waitFor(() => expect(f.n("pw_repo_structure")).toBe(2));
  expect(pill()).toHaveTextContent("4 specs ready");
});

test("a Refresh whose look at the specs failed still reads the clone again", async () => {
  const f = fake({ fails: "The spec files could not be read." });
  mount();
  await screen.findByText("Specs could not be read");
  await act(async () => {
    screen.getByRole("button", { name: "Refresh everything" }).click();
  });
  await waitFor(() => expect(f.n("pw_repo_structure")).toBe(2));
});

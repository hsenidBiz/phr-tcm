import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { resetAppSimpleModeStore, setAppSimpleModeForTest } from "../../../src/lib/appSimpleMode";
import AutoRun from "../../../src/screens/AutoRun";

// An unattended run going is the one input the switch waits on; the real
// store is replaced only for that answer.
const gate = vi.hoisted(() => ({ going: false }));
vi.mock("../../../src/lib/backgroundRun", async (orig) => {
  const real = await orig<typeof import("../../../src/lib/backgroundRun")>();
  return {
    ...real,
    unattendedRunGoing: () => gate.going,
    runBlockedReason: () => (gate.going ? real.RUN_GOING_REASON : undefined),
  };
});

afterEach(() => {
  gate.going = false;
  clearMocks();
  localStorage.clear();
  resetAppSimpleModeStore();
});

const PBI = { id: 42, title: "Login flow", work_item_type: "Product Backlog Item", project: "Web" };

function renderScreen() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="Web" pbi={PBI} />
    </QueryClientProvider>,
  );
}

test("the header switch offers Simple and Advanced and asks Rust for the one clicked", async () => {
  const calls: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "get_app_settings") return { autorun_mode: "simple" };
    if (cmd === "set_autorun_mode") {
      calls.push(args);
      return { autorun_mode: "advanced" };
    }
    if (cmd === "list_test_case_fields" || cmd === "pbi_test_cases_full") return [];
  });
  renderScreen();
  await waitFor(() => expect(screen.getByRole("button", { name: "Simple" })).toHaveAttribute("aria-pressed", "true"));
  fireEvent.click(screen.getByRole("button", { name: "Advanced" }));
  await waitFor(() => expect(calls).toEqual([{ mode: "advanced" }]));
  await waitFor(() => expect(screen.getByRole("button", { name: "Advanced" })).toHaveAttribute("aria-pressed", "true"));
});

test("the switch is disabled, with a reason, while an unattended run is going", () => {
  gate.going = true;
  mockIPC((cmd) => (cmd === "get_app_settings" ? { autorun_mode: "simple" } : []));
  renderScreen();
  const advanced = screen.getByRole("button", { name: "Advanced" });
  expect(advanced).toBeDisabled();
  expect(advanced).toHaveAttribute("title", expect.stringMatching(/run/i));
});

test("neither option is pressed, and clicks are ignored, until the settings answer", async () => {
  let answer!: (v: unknown) => void;
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "get_app_settings") return new Promise((r) => (answer = r));
    if (cmd === "list_test_case_fields" || cmd === "pbi_test_cases_full") return [];
  });
  renderScreen();
  const simple = screen.getByRole("button", { name: "Simple" });
  const advanced = screen.getByRole("button", { name: "Advanced" });
  expect(simple).toHaveAttribute("aria-pressed", "false");
  expect(advanced).toHaveAttribute("aria-pressed", "false");
  fireEvent.click(simple);
  expect(calls).not.toContain("set_autorun_mode");
  answer({ autorun_mode: "simple" });
  await waitFor(() => expect(screen.getByRole("button", { name: "Simple" })).toHaveAttribute("aria-pressed", "true"));
  expect(screen.getByRole("button", { name: "Advanced" })).toHaveAttribute("aria-pressed", "false");
});

test("with the app in Simple mode the switch offers only Simple, held pressed", async () => {
  setAppSimpleModeForTest(true);
  mockIPC((cmd) => {
    if (cmd === "get_app_settings") return { autorun_mode: "simple", app_simple_mode: true };
    if (cmd === "list_test_case_fields" || cmd === "pbi_test_cases_full") return [];
  });
  renderScreen();
  await waitFor(() => expect(screen.getByRole("button", { name: "Simple" })).toHaveAttribute("aria-pressed", "true"));
  expect(screen.queryByRole("button", { name: "Advanced" })).not.toBeInTheDocument();
});

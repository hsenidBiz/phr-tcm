// Auto Test's settings dialog: its name, and that the page size still saves
// from inside it through the same command Settings used.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import AutoRunSettingsDialog from "../../../src/screens/AutoRun/AutoRunSettingsDialog";

vi.mock("../../../src/lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

function mount() {
  const calls: unknown[] = [];
  let page_size = { width: 1280, height: 720 };
  mockIPC((cmd, args) => {
    if (cmd === "get_app_settings") return { close_to_tray: true, page_size };
    if (cmd === "get_autorun_browsers") return 5;
    if (cmd === "set_page_size") {
      calls.push(args);
      page_size = (args as { size: typeof page_size }).size;
      return page_size;
    }
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <AutoRunSettingsDialog onClose={() => {}} />
    </QueryClientProvider>,
  );
  return calls;
}

test("the dialog is named Auto Test settings", () => {
  mount();
  expect(screen.getByRole("dialog", { name: "Auto Test settings" })).toBeInTheDocument();
});

test("the page size saves from inside the dialog through set_page_size", async () => {
  const calls = mount();
  const width = () => screen.getByRole("spinbutton", { name: "Width" }) as HTMLInputElement;
  await waitFor(() => expect(width()).not.toBeDisabled());
  fireEvent.change(width(), { target: { value: "1600" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(calls).toEqual([{ size: { width: 1600, height: 720 } }]));
});

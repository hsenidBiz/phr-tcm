import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import BrowserLimitSettings from "../../src/components/BrowserLimitSettings";

vi.mock("../../src/lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

function rust(start: number) {
  const calls: unknown[] = [];
  let autorun_browsers = start;
  mockIPC((cmd, args) => {
    // Rust answers the clamped value, whatever the file holds.
    if (cmd === "get_autorun_browsers") return Math.min(Math.max(autorun_browsers, 1), 8);
    if (cmd === "set_autorun_browsers") {
      calls.push(args);
      autorun_browsers = (args as { count: number }).count;
      return autorun_browsers;
    }
    return undefined;
  });
  return calls;
}

function renderRow() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <BrowserLimitSettings />
    </QueryClientProvider>,
  );
}

const box = () => screen.getByRole("spinbutton", { name: "Browsers at once" }) as HTMLInputElement;
const save = () => screen.getByRole("button", { name: "Save: browsers at once" });

test("it names itself, explains itself and shows what Rust has", async () => {
  rust(6);
  renderRow();
  expect(screen.getByText("How many Auto Test browsers can be open at the same time. Each one uses memory.")).toBeInTheDocument();
  await waitFor(() => expect(box()).toHaveValue(6));
  expect(save()).toBeDisabled();
});

test("Save sends the number to set_autorun_browsers", async () => {
  const calls = rust(5);
  renderRow();
  await waitFor(() => expect(box()).not.toBeDisabled());
  fireEvent.change(box(), { target: { value: "8" } });
  fireEvent.click(save());
  await waitFor(() => expect(calls).toEqual([{ count: 8 }]));
  await waitFor(() => expect(save()).toBeDisabled());
  expect(box()).toHaveValue(8);
});

test("a number outside 1 to 8, or not whole, is refused inline and nothing is sent", async () => {
  const calls = rust(5);
  renderRow();
  await waitFor(() => expect(box()).not.toBeDisabled());
  for (const bad of ["0", "9", "2.5", ""]) {
    fireEvent.change(box(), { target: { value: bad } });
    fireEvent.click(save());
    expect(await screen.findByRole("alert")).toHaveTextContent("whole number from 1 to 8");
  }
  expect(calls).toEqual([]);
  fireEvent.change(box(), { target: { value: "1" } });
  fireEvent.click(save());
  await waitFor(() => expect(calls).toEqual([{ count: 1 }]));
  await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
});

test("a stored 40 shows 8, the value Rust holds it to", async () => {
  rust(40);
  renderRow();
  await waitFor(() => expect(box()).toHaveValue(8));
  expect(save()).toBeDisabled();
});

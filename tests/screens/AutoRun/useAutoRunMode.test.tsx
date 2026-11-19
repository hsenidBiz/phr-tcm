import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, test } from "vitest";
import { useAutoRunMode } from "../../../src/screens/AutoRun/useAutoRunMode";

afterEach(() => clearMocks());

function wrapper() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return ({ children }: { children: ReactNode }) => <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

test("a setting that was never written reads as advanced", async () => {
  mockIPC((cmd) => (cmd === "get_app_settings" ? { autorun_mode: null } : undefined));
  const { result } = renderHook(() => useAutoRunMode(), { wrapper: wrapper() });
  expect(result.current.mode).toBe("advanced");
  await waitFor(() => expect(result.current.mode).toBe("advanced"));
});

test("setMode asks Rust and the hook follows the answer", async () => {
  const calls: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "get_app_settings") return { autorun_mode: "advanced" };
    if (cmd === "set_autorun_mode") {
      calls.push(args);
      return { autorun_mode: "simple" };
    }
  });
  const { result } = renderHook(() => useAutoRunMode(), { wrapper: wrapper() });
  await act(() => result.current.setMode("simple"));
  expect(calls).toEqual([{ mode: "simple" }]);
  await waitFor(() => expect(result.current.mode).toBe("simple"));
});

test("loading is true until the settings have answered", async () => {
  mockIPC((cmd) => (cmd === "get_app_settings" ? { autorun_mode: "simple" } : undefined));
  const { result } = renderHook(() => useAutoRunMode(), { wrapper: wrapper() });
  expect(result.current.loading).toBe(true);
  await waitFor(() => expect(result.current.loading).toBe(false));
  expect(result.current.mode).toBe("simple");
});

test("the app's Basic mode holds the mode at simple, whatever an older save says", async () => {
  mockIPC((cmd) => (cmd === "get_app_settings" ? { autorun_mode: "advanced", app_simple_mode: true } : undefined));
  const { result } = renderHook(() => useAutoRunMode(), { wrapper: wrapper() });
  await waitFor(() => expect(result.current.loading).toBe(false));
  expect(result.current.mode).toBe("simple");
});

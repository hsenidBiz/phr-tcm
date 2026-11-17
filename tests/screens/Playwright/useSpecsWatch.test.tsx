// The specs watch while a run holds the Playwright slot: a refused re-read is
// not a failure, never loops on the re-read's own fetching, and goes again
// once, when the run ends.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { getSpecsPill } from "../../../src/lib/pwSpecs";
import { useSpecsWatch } from "../../../src/screens/Playwright/useSpecsWatch";

afterEach(() => {
  clearMocks();
});

const BUSY = "Playwright run is still running. Stop it, or wait for it to finish, before starting another.";
const pipeline = { found: [".claude/agents/spec-editor.md"], missing: [], fingerprint: "r" };

function setup(fingerprints: string[]) {
  const seen = [...fingerprints];
  mockIPC((cmd) => {
    if (cmd === "pw_repo_specs_status") return { count: 3, fingerprint: seen.length > 1 ? seen.shift() : seen[0], pipeline };
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
  return wrapper;
}

type Props = { busy: boolean; slotBusy: boolean };

test("a refused re-read does not loop on the fetching it causes, and goes again once when the run ends", async () => {
  const process = vi.fn<() => Promise<void>>().mockRejectedValue(new Error(BUSY));
  const wrapper = setup(["a", "b"]);
  const { result, rerender } = renderHook(
    ({ busy, slotBusy }: Props) => useSpecsWatch({ clone: "C:/repo", ready: true, busy, slotBusy, process }),
    { wrapper, initialProps: { busy: false, slotBusy: false } },
  );
  // The first look settles on "a"; a focus look sees "b" and re-reads, refused.
  await waitFor(() => expect(getSpecsPill()).not.toBeNull());
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  await waitFor(() => expect(process).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(getSpecsPill()?.label).toBe("Run in progress"));
  expect(getSpecsPill()?.tone).toBe("warning");

  // The re-read's own fetching comes and goes several times: nothing more is asked.
  for (let i = 0; i < 4; i++) {
    rerender({ busy: true, slotBusy: false });
    rerender({ busy: false, slotBusy: false });
  }
  await act(async () => {});
  expect(process).toHaveBeenCalledTimes(1);

  // A run starts and ends: one more re-read, which now succeeds.
  process.mockResolvedValue(undefined);
  rerender({ busy: true, slotBusy: true });
  rerender({ busy: false, slotBusy: false });
  await waitFor(() => expect(process).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(getSpecsPill()?.label).not.toBe("Run in progress"));
  expect(result.current.reprocess).toBeTypeOf("function");
  await act(async () => {});
  expect(process).toHaveBeenCalledTimes(2);
});

test("Refresh during a run with no spec change says a run is in progress, and clears when it ends", async () => {
  const process = vi.fn<() => Promise<void>>().mockRejectedValue(new Error(BUSY));
  const wrapper = setup(["a"]);
  const { result, rerender } = renderHook(
    ({ busy, slotBusy }: Props) => useSpecsWatch({ clone: "C:/repo", ready: true, busy, slotBusy, process }),
    { wrapper, initialProps: { busy: false, slotBusy: true } },
  );
  await waitFor(() => expect(getSpecsPill()).not.toBeNull());
  await act(async () => {
    await result.current.reprocess();
  });
  expect(getSpecsPill()).toMatchObject({ label: "Run in progress", tone: "warning" });
  expect(getSpecsPill()?.why).not.toMatch(/changed/);

  process.mockResolvedValue(undefined);
  rerender({ busy: false, slotBusy: false });
  await waitFor(() => expect(getSpecsPill()?.label).not.toBe("Run in progress"));
});

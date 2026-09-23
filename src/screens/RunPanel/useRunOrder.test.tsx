import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { toast } from "sonner";
import { afterEach, expect, test, vi } from "vitest";
import type { TestPoint } from "../../bindings";
import { useRunOrder } from "./useRunOrder";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const point = (point_id: number, test_case_id: number, test_case_name: string): TestPoint => ({
  point_id,
  test_case_id,
  test_case_name,
  config_name: "Windows 10",
  tester: "",
  last_outcome: "",
  last_run_id: null,
  last_result_id: null,
});

// Constants, so the hook's memos see the same arrays on every render.
const POINTS = [point(1, 301, "Alpha check"), point(2, 302, "Bravo check"), point(3, 303, "Charlie check")];
const SUITE = { plan_id: 9, suite_id: 91 };
const MY_KEY = "tcm-v2-run-order:acme/9/91";
const VIEW_KEY = "tcm-v2-run-order-view:acme/9/91";

function mountHook(runOrder: unknown = { state: "none" }, entries: number[] = [303, 301, 302]) {
  mockIPC((cmd) => {
    switch (cmd) {
      case "get_run_order":
        return runOrder;
      case "list_suite_entries":
        return entries.map((id, i) => ({ id, sequence_number: i + 1, entry_type: "testCase" }));
      case "list_test_points":
        // The suite-cases loader's own read: no names here, so a title in
        // specCases can only have come from the points the hook was given.
        return [];
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  );
  return renderHook(
    () => useRunOrder({ org: "acme", project: "Web", pbiId: 42, suite: SUITE, points: POINTS, grouped: false }),
    { wrapper },
  );
}

test("specCases is the suite's spec order, each case titled from its points", async () => {
  const { result } = mountHook();
  await waitFor(() =>
    expect(result.current.specCases).toEqual([
      { id: 303, title: "Charlie check" },
      { id: 301, title: "Alpha check" },
      { id: 302, title: "Bravo check" },
    ]),
  );
});

test("exposes the found file once the read settles", async () => {
  const FILE = {
    format: "tcm-run-order",
    version: 1,
    saved_by: "lead@example.com",
    saved_at: "2026-09-23T10:15:00Z",
    cases: [{ id: 302 }, { id: 301 }, { id: 303 }],
  };
  const { result } = mountHook({ state: "found", file: FILE });
  await waitFor(() => expect(result.current.loading).toBe(false));
  expect(result.current.file).toEqual(FILE);
  expect(result.current.view).toBe("suggested");
  expect(result.current.note).toBeNull();
});

test("saveMine stores the list as My order on this machine and makes it the list's order", async () => {
  const { result } = mountHook();
  await waitFor(() => expect(result.current.specCases.map((c) => c.id)).toEqual([303, 301, 302]));

  let ok = false;
  act(() => {
    ok = result.current.saveMine([302, 303, 301]);
  });

  expect(ok).toBe(true);
  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([302, 303, 301]);
  expect(localStorage.getItem(VIEW_KEY)).toBe("mine");
  expect(result.current.view).toBe("mine");
  expect(result.current.myOrder).toEqual([302, 303, 301]);
  expect(result.current.ordered.map((p) => p.test_case_id)).toEqual([302, 303, 301]);
  expect(toast.info).not.toHaveBeenCalled();
});

test("saveMine with storage unavailable says so and changes nothing", async () => {
  const { result } = mountHook();
  await waitFor(() => expect(result.current.loading).toBe(false));

  const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("QuotaExceededError");
  });
  let ok = true;
  try {
    act(() => {
      ok = result.current.saveMine([302, 303, 301]);
    });
  } finally {
    setItem.mockRestore();
  }

  expect(ok).toBe(false);
  expect(toast.error).toHaveBeenCalledWith("Your own order could not be saved on this machine.");
  expect(result.current.view).toBe("spec");
  expect(result.current.myOrder).toBeNull();
});

test("changeView switches the view and leaves My order stored", async () => {
  localStorage.setItem(MY_KEY, JSON.stringify([301, 302, 303]));
  localStorage.setItem(VIEW_KEY, "mine");
  const { result } = mountHook();
  await waitFor(() => expect(result.current.view).toBe("mine"));

  act(() => result.current.changeView("spec"));

  expect(result.current.view).toBe("spec");
  expect(result.current.myOrder).toEqual([301, 302, 303]);
  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([301, 302, 303]);
});

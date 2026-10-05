import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, test } from "vitest";
import { fetchPlan } from "./plan";

afterEach(() => clearMocks());

const plan = (order: number[]) => ({ order, phases: [order], resets: [], counts: null, saved: false });

test("a plan whose order is not these cases is null, so the caller keeps list order", async () => {
  mockIPC(() => plan([2, 7]));
  expect(await fetchPlan("o", "p", 1, [1, 2])).toBeNull();
  mockIPC(() => plan([2, 2]));
  expect(await fetchPlan("o", "p", 1, [1, 2])).toBeNull();
  mockIPC(() => plan([2]));
  expect(await fetchPlan("o", "p", 1, [1, 2])).toBeNull();
});

test("a plan that is a reordering of the cases is returned as it is", async () => {
  mockIPC(() => plan([2, 1]));
  expect((await fetchPlan("o", "p", 1, [1, 2]))?.order).toEqual([2, 1]);
});

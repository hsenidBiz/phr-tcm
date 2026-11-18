import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const setSwitchMarkersMigrated = vi.fn();
const getAppSettings = vi.fn();

vi.mock("../../src/bindings", () => ({
  commands: {
    getAppSettings: () => getAppSettings(),
    setSwitchMarkersMigrated: () => setSwitchMarkersMigrated(),
  },
}));

// The real list comes from Rust through several calls; the hook only needs
// the shared query, so the test fills it directly.
vi.mock("../../src/lib/environments", () => ({
  envKeys: { list: ["environments"] },
  useEnvironments: () => useQuery({ queryKey: ["environments"], queryFn: () => new Promise(() => {}), staleTime: Infinity }),
}));

import { saveApiWrites } from "../../src/lib/apiTemplates";
import { setActiveEnvId } from "../../src/lib/activeEnv";
import { markAllAsSet, turnOnUnsetSwitches, useSimpleModeSwitches } from "../../src/lib/simpleModeSwitches";

const SETTINGS = ["app-settings"];
const ENVS = ["environments"];

const settings = (over: object) => ({ app_simple_mode: false, switch_markers_migrated: true, ...over });
const list = (...ids: string[]) => ({ active: ids[0], environments: ids.map((id) => ({ id, name: id })) });
const api = (id: string) => localStorage.getItem(`tcm-v2-api-writes:${id}`);
const net = (id: string) => localStorage.getItem(`tcm-v2-network-tools:${id}`);

let qc: QueryClient;
const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={qc}>{children}</QueryClientProvider>;

beforeEach(() => {
  qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  setSwitchMarkersMigrated.mockReset();
  getAppSettings.mockReset();
  getAppSettings.mockReturnValue(new Promise(() => {}));
});
afterEach(() => localStorage.clear());

function mount(s: object, ids: string[]) {
  qc.setQueryData(SETTINGS, s);
  qc.setQueryData(ENVS, list(...ids));
  return renderHook(() => useSimpleModeSwitches(), { wrapper });
}
const update = (key: string[], v: unknown) =>
  act(async () => {
    qc.setQueryData(key, v);
    await new Promise((r) => setTimeout(r, 0));
  });

test("markAllAsSet writes both markers and leaves the switches alone", () => {
  localStorage.setItem("tcm-v2-api-writes:a", "1");
  markAllAsSet(["a", "b"]);
  for (const id of ["a", "b"]) {
    expect(localStorage.getItem(`tcm-v2-api-writes-set:${id}`)).toBe("1");
    expect(localStorage.getItem(`tcm-v2-network-tools-set:${id}`)).toBe("1");
  }
  expect(api("a")).toBe("1");
  expect(api("b")).toBeNull();
  expect(net("a")).toBeNull();
});

test("turnOnUnsetSwitches changes only environments without a marker and counts them", () => {
  localStorage.setItem("tcm-v2-api-writes-set:a", "1");
  expect(turnOnUnsetSwitches(["a", "b"])).toEqual({ apiWrites: 1, networkTools: 2 });
  expect(api("a")).toBeNull();
  expect(api("b")).toBe("1");
  expect(net("a")).toBe("1");
  expect(net("b")).toBe("1");
});

test("existing_environments_keep_their_switches_after_turning_simple_on", async () => {
  setSwitchMarkersMigrated.mockResolvedValue({
    status: "ok",
    data: settings({ switch_markers_migrated: true, app_simple_mode: false }),
  });
  mount(settings({ switch_markers_migrated: false, app_simple_mode: false }), ["A", "B"]);
  await act(async () => {});
  expect(setSwitchMarkersMigrated).toHaveBeenCalledTimes(1);

  await update(SETTINGS, settings({ app_simple_mode: true }));
  expect(api("A")).toBeNull();
  expect(net("B")).toBeNull();

  await update(ENVS, list("A", "B", "C"));
  expect(api("C")).toBe("1");
  expect(net("C")).toBe("1");
  expect(api("A")).toBeNull();
  expect(net("B")).toBeNull();
  expect(setSwitchMarkersMigrated).toHaveBeenCalledTimes(1);
});

test("a new install turns both switches on for Default", async () => {
  mount(settings({ app_simple_mode: true }), ["Default"]);
  await act(async () => {});
  expect(api("Default")).toBe("1");
  expect(net("Default")).toBe("1");
  expect(setSwitchMarkersMigrated).not.toHaveBeenCalled();
});

test("with Simple mode off a new environment gets nothing switched on", async () => {
  mount(settings({ app_simple_mode: false }), ["A"]);
  await update(ENVS, list("A", "C"));
  expect(api("C")).toBeNull();
  expect(net("C")).toBeNull();
});

test("a switch a person turned off stays off after the next list change", async () => {
  mount(settings({ app_simple_mode: true }), ["A"]);
  await act(async () => {});
  await update(ENVS, list("A", "C"));
  expect(api("C")).toBe("1");
  setActiveEnvId("C");
  act(() => saveApiWrites(false));
  expect(api("C")).toBeNull();
  await update(ENVS, list("A", "C", "D"));
  expect(api("C")).toBeNull();
  expect(api("D")).toBe("1");
});

test("nothing happens before the settings have loaded", async () => {
  qc.setQueryData(ENVS, list("A"));
  renderHook(() => useSimpleModeSwitches(), { wrapper });
  await act(async () => {});
  expect(api("A")).toBeNull();
  expect(setSwitchMarkersMigrated).not.toHaveBeenCalled();
});

import { afterEach, expect, test, vi } from "vitest";

import { setActiveEnvId } from "./activeEnv";
import { loadNetworkTools, networkToolsSnapshot, saveNetworkTools, subscribeNetworkTools } from "./networkTools";

afterEach(() => localStorage.clear());

/// Off until it is switched on, for a profile that never set it and for a
/// cleared one alike - the same rule as the API templates switch.
test("the network tools are off until they are switched on", () => {
  expect(loadNetworkTools()).toBe(false);
  expect(networkToolsSnapshot()).toBe(false);

  saveNetworkTools(true);
  expect(localStorage.getItem("tcm-v2-network-tools")).toBe("1");
  expect(networkToolsSnapshot()).toBe(true);

  saveNetworkTools(false);
  // Absent rather than "0": a cleared profile reads exactly as a fresh one.
  expect(localStorage.getItem("tcm-v2-network-tools")).toBeNull();
  expect(loadNetworkTools()).toBe(false);
});

/// Each environment keeps its own: switched on in one, another - and a new
/// one - reads off.
test("each environment keeps its own network tools switch", () => {
  setActiveEnvId("env-a");
  saveNetworkTools(true);
  expect(localStorage.getItem("tcm-v2-network-tools:env-a")).toBe("1");
  setActiveEnvId("env-b");
  expect(networkToolsSnapshot()).toBe(false);
  setActiveEnvId("env-a");
  expect(networkToolsSnapshot()).toBe(true);
  setActiveEnvId("");
});

test("saving notifies subscribers, so App can re-push the bridge context", () => {
  const seen = vi.fn();
  const stop = subscribeNetworkTools(seen);
  saveNetworkTools(true);
  expect(seen).toHaveBeenCalledTimes(1);
  saveNetworkTools(false);
  expect(seen).toHaveBeenCalledTimes(2);
  stop();
  saveNetworkTools(true);
  expect(seen).toHaveBeenCalledTimes(2);
});

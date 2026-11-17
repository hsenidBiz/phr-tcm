import { afterEach, expect, test, vi } from "vitest";

import { setActiveEnvId } from "../../src/lib/activeEnv";
import { loadNetworkTools, networkToolsSnapshot, saveNetworkTools, subscribeNetworkTools, turnOnNetworkToolsWhereUnset } from "../../src/lib/networkTools";

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

test("turning the tools on where unset leaves an environment a person chose alone", () => {
  localStorage.setItem("tcm-v2-network-tools-set:env-a", "1");
  expect(turnOnNetworkToolsWhereUnset(["env-a", "env-b"])).toBe(1);
  expect(localStorage.getItem("tcm-v2-network-tools:env-a")).toBeNull();
  expect(localStorage.getItem("tcm-v2-network-tools:env-b")).toBe("1");
  // A default, not a choice: no marker is written.
  expect(localStorage.getItem("tcm-v2-network-tools-set:env-b")).toBeNull();
});

test("saving the switch off is a person's choice that a later switch-on respects", () => {
  setActiveEnvId("env-a");
  saveNetworkTools(false);
  expect(localStorage.getItem("tcm-v2-network-tools-set:env-a")).toBe("1");
  expect(turnOnNetworkToolsWhereUnset(["env-a"])).toBe(0);
  expect(localStorage.getItem("tcm-v2-network-tools:env-a")).toBeNull();
  setActiveEnvId("");
});

test("an environment already on is not counted as changed", () => {
  localStorage.setItem("tcm-v2-network-tools:env-a", "1");
  expect(turnOnNetworkToolsWhereUnset(["env-a"])).toBe(0);
});

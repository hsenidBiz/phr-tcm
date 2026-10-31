// An editor open over one environment's files must not save them into
// another: it closes, unsaved, when the active environment changes.

import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { setActiveEnvId } from "../lib/activeEnv";
import { toast } from "../lib/toast";
import { useCloseOnEnvironmentChange } from "./useCloseOnEnvironmentChange";

vi.mock("../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));

beforeEach(() => setActiveEnvId("env-00000001"));
afterEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
});

function Probe({ onClose }: { onClose: () => void }) {
  useCloseOnEnvironmentChange(onClose);
  return null;
}

test("closes once when the active environment changes, never on mount", () => {
  const onClose = vi.fn();
  render(<Probe onClose={onClose} />);
  expect(onClose).not.toHaveBeenCalled();
  act(() => setActiveEnvId("env-00000002"));
  expect(onClose).toHaveBeenCalledTimes(1);
  expect(toast.info).toHaveBeenCalledWith("The environment changed, so this was closed without saving.");
  // The same environment again is not a change.
  act(() => setActiveEnvId("env-00000002"));
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("opening before any environment is known does not close when the first one arrives", () => {
  localStorage.clear();
  const onClose = vi.fn();
  render(<Probe onClose={onClose} />);
  act(() => setActiveEnvId("env-00000001"));
  expect(onClose).not.toHaveBeenCalled();
});

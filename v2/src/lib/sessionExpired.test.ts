// The session-expired latch: raised by the one formatter every shown ADO
// error passes through, cleared by re-sign-in or dismissal.

import { beforeEach, expect, test, vi } from "vitest";
import type { AdoError } from "../bindings";
import { describeAdoError } from "./ipc";
import {
  clearSessionExpired,
  flagSessionExpired,
  sessionExpiredSnapshot,
  subscribeSessionExpired,
} from "./sessionExpired";

beforeEach(() => clearSessionExpired());

test("formatting an Unauthorized error raises the shared flag", () => {
  expect(sessionExpiredSnapshot()).toBe(false);
  const msg = describeAdoError({ kind: "Unauthorized" } as AdoError);
  expect(msg).toMatch(/session has expired/i);
  expect(sessionExpiredSnapshot()).toBe(true);
});

test("other error kinds do not touch the flag", () => {
  describeAdoError({ kind: "Forbidden" } as AdoError);
  describeAdoError({ kind: "NotFound" } as AdoError);
  describeAdoError({ kind: "Network", detail: "down" } as AdoError);
  expect(sessionExpiredSnapshot()).toBe(false);
});

test("subscribers hear transitions but not repeats", () => {
  const heard = vi.fn();
  const off = subscribeSessionExpired(heard);
  flagSessionExpired();
  flagSessionExpired(); // already true - a latch, not a counter
  expect(heard).toHaveBeenCalledTimes(1);
  clearSessionExpired();
  expect(heard).toHaveBeenCalledTimes(2);
  off();
  flagSessionExpired();
  expect(heard).toHaveBeenCalledTimes(2);
});

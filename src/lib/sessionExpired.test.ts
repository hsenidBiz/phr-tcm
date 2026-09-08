// The session-expired latch: raised by the one formatter every shown ADO
// error passes through, cleared by re-sign-in or dismissal - and ARMED
// only while a session exists, because before sign-in the backend answers
// everything with Unauthorized and none of those are expiries.

import { beforeEach, expect, test, vi } from "vitest";
import type { AdoError } from "../bindings";
import { describeAdoError } from "./ipc";
import {
  clearSessionExpired,
  flagSessionExpired,
  sessionExpiredSnapshot,
  setSessionActive,
  subscribeSessionExpired,
} from "./sessionExpired";

beforeEach(() => {
  setSessionActive(true);
  clearSessionExpired();
});

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

/** The regression that shipped: a background query fired before the first
 * sign-in, the no-token Unauthorized flowed through the formatter, and the
 * "Session expired" modal greeted the user right after they signed in. */
test("an Unauthorized while signed out is not an expiry", () => {
  setSessionActive(false);
  describeAdoError({ kind: "Unauthorized" } as AdoError);
  expect(sessionExpiredSnapshot()).toBe(false);

  // Arming afterwards must not resurrect the ignored flag.
  setSessionActive(true);
  expect(sessionExpiredSnapshot()).toBe(false);
});

test("going inactive retires a raised latch", () => {
  flagSessionExpired();
  expect(sessionExpiredSnapshot()).toBe(true);
  setSessionActive(false);
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

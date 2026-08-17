// The unseen-assignments counter behind the Board rail badge.

import { beforeEach, expect, test, vi } from "vitest";
import {
  addWorkAlerts,
  clearWorkAlerts,
  subscribeWorkAlerts,
  workAlertsSnapshot,
} from "./workAlerts";

beforeEach(() => clearWorkAlerts());

test("assignments accumulate until the board is looked at", () => {
  addWorkAlerts(2);
  addWorkAlerts(1);
  expect(workAlertsSnapshot()).toBe(3);
  clearWorkAlerts();
  expect(workAlertsSnapshot()).toBe(0);
});

test("zero additions and repeat clears stay silent for subscribers", () => {
  const heard = vi.fn();
  const off = subscribeWorkAlerts(heard);
  addWorkAlerts(0); // an empty batch is not news
  clearWorkAlerts(); // already zero
  expect(heard).not.toHaveBeenCalled();
  addWorkAlerts(4);
  expect(heard).toHaveBeenCalledTimes(1);
  off();
});

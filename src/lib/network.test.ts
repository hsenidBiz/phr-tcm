import { afterEach, expect, test } from "vitest";
import { onlineSnapshot, subscribeOnline } from "./network";

/** jsdom starts online; each test puts the world back. */
function goOffline() {
  Object.defineProperty(navigator, "onLine", { value: false, configurable: true });
  window.dispatchEvent(new Event("offline"));
}
function goOnline() {
  Object.defineProperty(navigator, "onLine", { value: true, configurable: true });
  window.dispatchEvent(new Event("online"));
}

afterEach(goOnline);

test("the store follows the browser's online/offline events and notifies", () => {
  expect(onlineSnapshot()).toBe(true);
  const seen: boolean[] = [];
  const un = subscribeOnline(() => seen.push(onlineSnapshot()));

  goOffline();
  expect(onlineSnapshot()).toBe(false);
  goOnline();
  expect(onlineSnapshot()).toBe(true);
  expect(seen).toEqual([false, true]);

  // A repeated event is not a change and must not re-notify - the runner
  // flushes on this signal, and double-notifies would double the flush
  // attempts (harmless but noisy).
  goOnline();
  expect(seen).toEqual([false, true]);
  un();
});

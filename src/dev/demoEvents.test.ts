// Demo mode must keep a REAL session's pushed events off the screen: the
// Rust assigned-work poller keeps polling the real org if the dev app was
// signed in before demo mode went on, and its payload carries real
// work-item titles.

import { afterEach, beforeEach, expect, test, vi } from "vitest";

const tauriListen = vi.hoisted(() => vi.fn(() => Promise.resolve(() => {})));
vi.mock("@tauri-apps/api/event", () => ({
  listen: tauriListen,
  once: tauriListen,
  emit: vi.fn(() => Promise.resolve()),
}));

beforeEach(() => {
  vi.resetModules();
  tauriListen.mockClear();
});
afterEach(() => localStorage.clear());

async function boot(demo: boolean) {
  if (demo) localStorage.setItem("tcm-v2-dev-demo", "on");
  const { maybeEnableDemoMode, MUTED_EVENTS } = await import("./demo");
  const { events } = await import("../bindings");
  maybeEnableDemoMode();
  return { events, MUTED_EVENTS };
}

test("in demo mode, assigned work and slow-down events never reach a listener", async () => {
  const { events, MUTED_EVENTS } = await boot(true);
  expect([...MUTED_EVENTS]).toEqual(["workAssigned", "slowdownRequested"]);
  const cb = vi.fn();
  for (const name of MUTED_EVENTS) {
    const un = await events[name].listen(cb);
    expect(typeof un).toBe("function");
    un();
    await events[name].once(cb);
  }
  expect(tauriListen).not.toHaveBeenCalled();
  expect(cb).not.toHaveBeenCalled();
});

test("in demo mode, events from files on this machine still get through", async () => {
  const { events } = await boot(true);
  await events.watchedFileChanged.listen(() => {});
  await events.draftCommentSaved.listen(() => {});
  expect(tauriListen.mock.calls.map((c) => (c as unknown[])[0])).toEqual(["watched-file-changed", "draft-comment-saved"]);
});

test("outside demo mode, assigned work is listened for as normal", async () => {
  const { events } = await boot(false);
  await events.workAssigned.listen(() => {});
  expect(tauriListen.mock.calls.map((c) => (c as unknown[])[0])).toEqual(["work-assigned"]);
});

import { act, renderHook } from "@testing-library/react";
import { beforeEach, expect, test } from "vitest";
import { setTourRunning } from "../tour/tourState";
import { forgetScreenState, resetScreenStateForTests, useScreenKeys, useScreenState } from "./screenState";

beforeEach(() => {
  localStorage.clear();
  resetScreenStateForTests();
  setTourRunning(false);
});

test("a value survives an unmount and comes back on the next mount", () => {
  const first = renderHook(() => useScreenState("view:a/p/1:search", ""));
  act(() => first.result.current[1]("login"));
  first.unmount();
  const second = renderHook(() => useScreenState("view:a/p/1:search", ""));
  expect(second.result.current[0]).toBe("login");
});

test("session values never reach disk", () => {
  const { result } = renderHook(() => useScreenState("view:a/p/1:search", ""));
  act(() => result.current[1]("login"));
  expect(Object.keys(localStorage).filter((k) => k.startsWith("tcm-v2-screen:"))).toEqual([]);
});

test("a persisted value survives a restart and is removed when back at its start", () => {
  const { result, unmount } = renderHook(() => useScreenState("manual:a/1:title", "", { persist: true }));
  act(() => result.current[1]("Sign in"));
  expect(localStorage.getItem("tcm-v2-screen:manual:a/1:title")).toBe('"Sign in"');
  unmount();
  resetScreenStateForTests(); // a restart: memory is gone, disk is not
  const again = renderHook(() => useScreenState("manual:a/1:title", "", { persist: true }));
  expect(again.result.current[0]).toBe("Sign in");
  act(() => again.result.current[1](""));
  expect(localStorage.getItem("tcm-v2-screen:manual:a/1:title")).toBeNull();
});

test("a key change loads that key's value and never files one scope under another", () => {
  const { result, rerender } = renderHook(({ k }) => useScreenState(k, ""), {
    initialProps: { k: "view:a/p/1:search" },
  });
  act(() => result.current[1]("one"));
  rerender({ k: "view:a/p/2:search" });
  expect(result.current[0]).toBe("");
  act(() => result.current[1]("two"));
  rerender({ k: "view:a/p/1:search" });
  expect(result.current[0]).toBe("one");
});

test("a null key keeps nothing", () => {
  const first = renderHook(() => useScreenState<string>(null, ""));
  act(() => first.result.current[1]("x"));
  first.unmount();
  expect(renderHook(() => useScreenState<string>(null, "")).result.current[0]).toBe("");
});

test("keys are null while the tour runs", () => {
  setTourRunning(true);
  const { result } = renderHook(() => useScreenKeys("view:a/p/1"));
  expect(result.current("search")).toBeNull();
  setTourRunning(false);
});

test("forgetScreenState removes memory and disk values under a prefix", () => {
  const { result } = renderHook(() => useScreenState("manual:a/1:title", "", { persist: true }));
  act(() => result.current[1]("x"));
  forgetScreenState("manual:a/1:");
  expect(localStorage.getItem("tcm-v2-screen:manual:a/1:title")).toBeNull();
  expect(renderHook(() => useScreenState("manual:a/1:title", "")).result.current[0]).toBe("");
});

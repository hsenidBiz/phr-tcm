import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { usePersistedStringSet } from "./collapsedGroups";

afterEach(() => localStorage.clear());

test("toggling adds/removes and persists to localStorage", () => {
  const { result } = renderHook(() => usePersistedStringSet("k"));
  expect([...result.current[0]]).toEqual([]);

  act(() => result.current[1]("Login"));
  expect(result.current[0].has("Login")).toBe(true);
  expect(JSON.parse(localStorage.getItem("k")!)).toEqual(["Login"]);

  act(() => result.current[1]("Login"));
  expect(result.current[0].has("Login")).toBe(false);
  expect(JSON.parse(localStorage.getItem("k")!)).toEqual([]);
});

test("initializes from a previously persisted set", () => {
  localStorage.setItem("k", JSON.stringify(["Checkout", "Login"]));
  const { result } = renderHook(() => usePersistedStringSet("k"));
  expect(result.current[0].has("Checkout")).toBe(true);
  expect(result.current[0].has("Login")).toBe(true);
});

test("bad stored value falls back to empty", () => {
  localStorage.setItem("k", "not json");
  const { result } = renderHook(() => usePersistedStringSet("k"));
  expect([...result.current[0]]).toEqual([]);
});

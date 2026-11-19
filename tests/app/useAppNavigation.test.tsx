// Every way into a screen Simple mode hides opens Auto Test instead.
import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { useAppNavigation } from "../../src/app/useAppNavigation";
import { resetAppSimpleModeStore, setAppSimpleModeForTest } from "../../src/lib/appSimpleMode";

afterEach(() => resetAppSimpleModeStore());

function setup() {
  const setSection = vi.fn();
  const { result } = renderHook(() =>
    useAppNavigation({
      section: "manual",
      workMode: false,
      tourOpen: false,
      org: "acme",
      project: "Web",
      allProjects: false,
      setSection,
      setWorkMode: vi.fn(),
      setCaseSelection: vi.fn(),
      setRunFolder: vi.fn(),
      setProject: vi.fn(),
      setPbi: vi.fn(),
    }),
  );
  return { go: result.current.goToSection, setSection };
}

test("in Simple mode a jump to a hidden screen opens Auto Test; every other screen is untouched", () => {
  setAppSimpleModeForTest(true);
  const { go, setSection } = setup();
  for (const id of ["import", "edit", "suites", "manage", "apitemplates", "playwright"] as const) {
    act(() => go(id));
    expect(setSection).toHaveBeenLastCalledWith("autorun");
  }
  for (const id of ["manual", "run", "settings"] as const) {
    act(() => go(id));
    expect(setSection).toHaveBeenLastCalledWith(id);
  }
});

test("with Simple mode off a jump goes where it was sent", () => {
  const { go, setSection } = setup();
  act(() => go("apitemplates"));
  expect(setSection).toHaveBeenLastCalledWith("apitemplates");
});

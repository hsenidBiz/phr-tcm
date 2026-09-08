import { afterEach, expect, test, vi } from "vitest";
import {
  clearTourExpanded,
  publishSidebarChange,
  setTourExpanded,
  sidebarCollapsedSnapshot,
  stickyLeftPx,
  subscribeSidebar,
} from "./sidebarState";

const KEY = "tcm-v2-sidebar";

afterEach(() => {
  clearTourExpanded();
  localStorage.clear();
});

test("the tour override wins over a stored collapsed value and notifies", () => {
  localStorage.setItem(KEY, "collapsed");
  expect(sidebarCollapsedSnapshot()).toBe(true);

  const heard = vi.fn();
  const off = subscribeSidebar(heard);
  setTourExpanded(true);

  expect(sidebarCollapsedSnapshot()).toBe(false);
  expect(heard).toHaveBeenCalledTimes(1);
  off();
});

test("clearing the override restores the stored value and notifies", () => {
  localStorage.setItem(KEY, "collapsed");
  setTourExpanded(true);
  expect(sidebarCollapsedSnapshot()).toBe(false);

  const heard = vi.fn();
  const off = subscribeSidebar(heard);
  clearTourExpanded();

  expect(sidebarCollapsedSnapshot()).toBe(true);
  expect(heard).toHaveBeenCalledTimes(1);
  off();
});

test("clearing a second time when already off does not notify again", () => {
  const heard = vi.fn();
  const off = subscribeSidebar(heard);
  clearTourExpanded();
  expect(heard).not.toHaveBeenCalled();
  off();
});

test("the storage key is never written while the override is on", () => {
  localStorage.setItem(KEY, "collapsed");
  setTourExpanded(true);
  expect(sidebarCollapsedSnapshot()).toBe(false);
  expect(localStorage.getItem(KEY)).toBe("collapsed");
  setTourExpanded(false);
  expect(localStorage.getItem(KEY)).toBe("collapsed");
});

test("stickyLeftPx follows the collapsed flag it is given", () => {
  expect(stickyLeftPx(true)).toBeLessThan(stickyLeftPx(false));
});

test("publishSidebarChange still fans out to subscribers directly", () => {
  const heard = vi.fn();
  const off = subscribeSidebar(heard);
  publishSidebarChange();
  expect(heard).toHaveBeenCalledTimes(1);
  off();
});

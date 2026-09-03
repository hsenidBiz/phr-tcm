// The rail's badge bubbles: a count on an item when something arrived,
// nothing at all when the count is zero.

import { render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import Sidebar, { WORK_ITEMS, type WorkSection } from "./Sidebar";
import { clearTourExpanded, setTourExpanded } from "../lib/sidebarState";

afterEach(() => {
  clearTourExpanded();
  localStorage.clear();
});

test("the tour override renders the sidebar expanded over a stored collapsed setting", () => {
  localStorage.setItem("tcm-v2-sidebar", "collapsed");
  setTourExpanded(true);
  render(<Sidebar section="manual" onSelect={() => {}} />);
  // Expanded: the label text is present in the DOM (collapsed only fades it).
  expect(screen.getByText("Manual Entry")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Close sidebar" })).toBeInTheDocument();
});

test("a badge count lands on its item and reaches the accessible name", () => {
  render(
    <Sidebar<WorkSection>
      section="prs"
      onSelect={() => {}}
      items={WORK_ITEMS}
      badges={{ board: 3 }}
    />,
  );
  expect(screen.getByRole("button", { name: "Board (3 new)" })).toBeInTheDocument();
  expect(screen.getByText("3")).toBeInTheDocument();
});

/// Auto Run is a development-build tab: `tauri build` sets DEV false and
/// the row disappears; dev and test builds keep it. Fresh module import
/// each time, because the flag is read once at module load.
test("Auto Run is offered in dev builds and hidden in release builds", async () => {
  const { vi } = await import("vitest");

  vi.stubEnv("DEV", true);
  vi.resetModules();
  const dev = await import("./Sidebar");
  const { unmount } = render(<dev.default section="manual" onSelect={() => {}} />);
  expect(screen.getByRole("button", { name: /Auto Run/ })).toBeInTheDocument();
  unmount();

  vi.stubEnv("DEV", false);
  vi.resetModules();
  const release = await import("./Sidebar");
  render(<release.default section="manual" onSelect={() => {}} />);
  expect(screen.queryByRole("button", { name: /Auto Run/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Test Suites" })).toBeInTheDocument();

  vi.unstubAllEnvs();
  vi.resetModules();
});

test("zero and absent counts render no bubble", () => {
  render(
    <Sidebar<WorkSection>
      section="prs"
      onSelect={() => {}}
      items={WORK_ITEMS}
      badges={{ board: 0 }}
    />,
  );
  expect(screen.getByRole("button", { name: "Board" })).toBeInTheDocument();
  expect(screen.queryByText("0")).not.toBeInTheDocument();
});

// The rail's badge bubbles: a count on an item when something arrived,
// nothing at all when the count is zero.

import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import Sidebar, { WORK_ITEMS, sectionShortcut, shortcutOrder, visibleCaseItems, type WorkSection } from "./Sidebar";
import { clearTourExpanded, setTourExpanded } from "../lib/sidebarState";
import { ALL_MODULES, resetModulesStore, setModule, setModulesForTest, shouldLeaveHidden, type Module } from "../lib/modules";

afterEach(() => {
  clearTourExpanded();
  localStorage.clear();
  resetModulesStore();
  clearMocks();
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

/// Auto Run is still being built, and its row says so with an "In Dev"
/// pill after the label. The row keeps its plain name, so every shortcut,
/// tour stop and test that finds it by "Auto Run" still does.
test("Auto Run carries an In Dev pill, and no other row does", () => {
  setModulesForTest(ALL_MODULES);
  render(<Sidebar section="manual" onSelect={() => {}} />);
  const autoRun = screen.getByRole("button", { name: "Auto Run" });
  expect(autoRun).toHaveTextContent("In Dev");
  expect(screen.getAllByText("In Dev")).toHaveLength(1);
  expect(screen.getByRole("button", { name: "Run Tests" })).not.toHaveTextContent("In Dev");
});

/// Each module's rows appear when it is switched on and go again when it is
/// switched off - live, without a reload, because the rail subscribes to the
/// modules. A fresh install (none on) shows neither.
test("Auto Run, API Templates and Playwright follow their own modules, live", async () => {
  mockIPC((cmd) => (cmd === "modules_set" ? [] : null));
  render(<Sidebar section="manual" onSelect={() => {}} />);
  expect(screen.getByRole("button", { name: "Suite Management" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Auto Run/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /API Templates/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Playwright" })).not.toBeInTheDocument();
  expect(shortcutOrder()).not.toContain("autorun");

  act(() => setModulesForTest(["db_access", "auto_run"]));
  expect(screen.getByRole("button", { name: /Auto Run/ })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /API Templates/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Playwright" })).not.toBeInTheDocument();
  expect(shortcutOrder()).toContain("autorun");
  expect(sectionShortcut("autorun")).toBe("mod+6");

  act(() => setModulesForTest(["db_access", "auto_run", "api_templates"]));
  expect(screen.getByRole("button", { name: /API Templates/ })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Playwright" })).not.toBeInTheDocument();

  act(() => setModulesForTest(["db_access", "auto_run", "playwright"]));
  expect(screen.queryByRole("button", { name: /API Templates/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Playwright" })).toBeInTheDocument();
  // Playwright takes no Ctrl number, so the tabs below keep theirs.
  expect(shortcutOrder()).not.toContain("playwright");
  expect(sectionShortcut("playwright")).toBeUndefined();
  expect(sectionShortcut("ai")).toBe("mod+9");

  await act(() => setModule("db_access", false));
  expect(screen.queryByRole("button", { name: /Auto Run/ })).not.toBeInTheDocument();
  expect(sectionShortcut("autorun")).toBeUndefined();
  expect(shouldLeaveHidden("playwright", false, true)).toBe(true);
});

/// Capture mode (the screenshot script) reports every module on: the help
/// site documents Auto Run, API Templates and Playwright. Off, the rail
/// follows the modules.
test("capture mode shows every module's rows; off, the rail follows the modules", () => {
  localStorage.setItem("tcm-v2-dev-capture", "on");
  const { unmount } = render(<Sidebar section="manual" onSelect={() => {}} />);
  expect(screen.getByRole("button", { name: /Auto Run/ })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /API Templates/ })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Playwright" })).toBeInTheDocument();
  expect(shortcutOrder()).toContain("autorun");
  unmount();

  localStorage.removeItem("tcm-v2-dev-capture");
  render(<Sidebar section="manual" onSelect={() => {}} />);
  expect(screen.queryByRole("button", { name: /Auto Run/ })).not.toBeInTheDocument();
});

const ON = new Set<Module>(ALL_MODULES);
const NONE = new Set<Module>();

/// The Ctrl+N order closes up around a hidden Auto Run row, whichever way
/// the answer comes.
test("the shortcut order with and without Auto Run", () => {
  expect(shortcutOrder(ON)).toEqual([
    "manual",
    "import",
    "edit",
    "view",
    "run",
    "autorun",
    "suites",
    "manage",
    "ai",
    "apitemplates",
  ]);
  expect(shortcutOrder(NONE)).toEqual(["manual", "import", "edit", "view", "run", "suites", "manage", "ai"]);
});

/// Playwright sits right under Auto Run in the rail, but has no Ctrl number,
/// so Search Suites, Suite Management and AI Bridge keep theirs.
test("Playwright sits under Auto Run and moves no one's Ctrl number", () => {
  const rows = visibleCaseItems(ON).map((i) => i.id);
  expect(rows.indexOf("playwright")).toBe(rows.indexOf("autorun") + 1);
  expect(sectionShortcut("playwright", ON)).toBeUndefined();
  expect(sectionShortcut("suites", ON)).toBe("mod+7");
  expect(sectionShortcut("manage", ON)).toBe("mod+8");
  expect(sectionShortcut("ai", ON)).toBe("mod+9");
});

/// API Templates sits after AI Bridge so every existing tab keeps its
/// Ctrl+number, and it gets none of its own: App's handler only reaches
/// Ctrl+1..9, and a hint for a key that does nothing would be a lie.
test("API Templates comes last and has no Ctrl number", () => {
  expect(sectionShortcut("apitemplates", ON)).toBeUndefined();
  expect(sectionShortcut("ai", ON)).toBe("mod+9");
  expect(sectionShortcut("autorun", ON)).toBe("mod+6");
  expect(sectionShortcut("apitemplates", NONE)).toBeUndefined();
});

/// API Templates and Playwright are separate modules under Auto Run: either
/// can be on without the other.
test("API Templates and Playwright are separate rows", () => {
  const base: Module[] = ["db_access", "auto_run"];
  expect(visibleCaseItems(new Set<Module>([...base, "api_templates"])).map((i) => i.id)).toContain("apitemplates");
  expect(visibleCaseItems(new Set<Module>([...base, "api_templates"])).map((i) => i.id)).not.toContain("playwright");
  expect(visibleCaseItems(new Set<Module>([...base, "playwright"])).map((i) => i.id)).toContain("playwright");
  expect(visibleCaseItems(new Set<Module>([...base, "playwright"])).map((i) => i.id)).not.toContain("apitemplates");
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

// The guided tour moves the rail OUT of the app's inert region so the one
// tab it is asking for can be clicked. Everything else in the rail has to
// lock itself, including the collapse toggle - which writes the user's own
// stored setting, and the tour promised not to touch it.

test("while the tour runs only the row it is waiting for answers", () => {
  const picked: string[] = [];
  render(<Sidebar section="manual" onSelect={(s) => picked.push(s)} locked liveItem="import" />);

  const importRow = screen.getByRole("button", { name: "Import Test Cases" });
  const update = screen.getByRole("button", { name: "Update Test Cases" });
  expect(importRow).toBeEnabled();
  expect(update).toBeDisabled();

  fireEvent.click(update);
  fireEvent.keyDown(update, { key: "Enter" });
  expect(picked).toEqual([]);

  fireEvent.click(importRow);
  expect(picked).toEqual(["import"]);
});

test("the collapse toggle is dead while the tour runs, and writes nothing", () => {
  const wrote: string[] = [];
  const setItem = Storage.prototype.setItem;
  const spy = vi
    .spyOn(Storage.prototype, "setItem")
    .mockImplementation(function (this: Storage, k: string, v: string) {
      wrote.push(k);
      setItem.call(this, k, v);
    });
  try {
    setTourExpanded(true);
    render(<Sidebar section="manual" onSelect={() => {}} locked />);
    const toggle = screen.getByRole("button", { name: "Close sidebar" });
    expect(toggle).toBeDisabled();
    fireEvent.click(toggle);
    expect(wrote).not.toContain("tcm-v2-sidebar");
    // ...and it is still expanded, so nothing moved behind the overlay.
    expect(screen.getByRole("button", { name: "Close sidebar" })).toBeInTheDocument();
  } finally {
    spy.mockRestore();
  }
});

test("nothing is locked when the tour is not running", () => {
  render(<Sidebar section="manual" onSelect={() => {}} />);
  expect(screen.getByRole("button", { name: "Update Test Cases" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Close sidebar" })).toBeEnabled();
});

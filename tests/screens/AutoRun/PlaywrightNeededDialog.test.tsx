// The "Playwright is needed" dialog: Auto Run says once per app start that
// the assistant is on the shipped rules because no Playwright repo is set,
// and offers the way to set one - the Playwright tab, or Modules when that
// module is off. The Modal portals to document.body, so the a11y audit
// here runs over the body.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { resetAppSimpleModeStore, setAppSimpleModeForTest } from "../../../src/lib/appSimpleMode";
import { resetModulesStore, setModulesForTest, useModule } from "../../../src/lib/modules";
import PlaywrightNeededDialog, { resetPlaywrightNoticeForTests, usePlaywrightNotice } from "../../../src/screens/AutoRun/PlaywrightNeededDialog";

beforeEach(() => {
  resetPlaywrightNoticeForTests();
  resetModulesStore();
});
afterEach(() => {
  cleanup();
  resetAppSimpleModeStore();
  clearMocks();
  resetModulesStore();
});

function answer(clone_set: boolean, clone_gone = false) {
  mockIPC((cmd) => (cmd === "pw_rules_status" ? { clone_set, clone_gone } : undefined));
}

/** What AutoRun does with the hook, with the module state the test wants. */
function Host({
  onOpenPlaywright = () => {},
  onOpenModules = () => {},
}: {
  onOpenPlaywright?: () => void;
  onOpenModules?: () => void;
}) {
  const notice = usePlaywrightNotice();
  const playwrightOn = useModule("playwright");
  return notice.open ? (
    <PlaywrightNeededDialog
      playwrightOn={playwrightOn}
      gone={notice.gone}
      onOpenPlaywright={onOpenPlaywright}
      onOpenModules={onOpenModules}
      onClose={notice.close}
    />
  ) : (
    <p>nothing to say</p>
  );
}

function mount(host: React.ReactElement = <Host />) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(<QueryClientProvider client={qc}>{host}</QueryClientProvider>);
  return { view, qc };
}

test("says Playwright is needed and opens the Playwright tab", async () => {
  setModulesForTest(["playwright"]);
  answer(false);
  const onOpenPlaywright = vi.fn();
  mount(<Host onOpenPlaywright={onOpenPlaywright} />);
  const dialog = await screen.findByRole("dialog", { name: "Playwright is needed" });
  expect(dialog).toHaveTextContent("rules shipped with this version");
  expect(screen.queryByText(/The Playwright module is off/)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Open the Playwright tab" }));
  expect(onOpenPlaywright).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("says no repo is set when none ever was", async () => {
  setModulesForTest(["playwright"]);
  answer(false, false);
  mount();
  const dialog = await screen.findByRole("dialog", { name: "Playwright is needed" });
  expect(dialog).toHaveTextContent("No Playwright repo is set on the Playwright tab, so the assistant is using the rules shipped");
  expect(dialog).not.toHaveTextContent("no longer there");
});

test("says the repo is no longer there when one is set but its folder is gone", async () => {
  setModulesForTest(["playwright"]);
  answer(false, true);
  mount();
  const dialog = await screen.findByRole("dialog", { name: "Playwright is needed" });
  expect(dialog).toHaveTextContent(
    "The Playwright repo set on the Playwright tab is no longer there, so the assistant is using the rules shipped",
  );
  expect(dialog).not.toHaveTextContent("No Playwright repo is set");
  // The way forward is the same: the Playwright tab.
  expect(screen.getByRole("button", { name: "Open the Playwright tab" })).toBeInTheDocument();
});

test("with the Playwright module off it opens Modules", async () => {
  setModulesForTest([]);
  answer(false);
  const onOpenModules = vi.fn();
  const onOpenPlaywright = vi.fn();
  mount(<Host onOpenModules={onOpenModules} onOpenPlaywright={onOpenPlaywright} />);
  const dialog = await screen.findByRole("dialog", { name: "Playwright is needed" });
  expect(dialog).toHaveTextContent("The Playwright module is off: switch it on in Modules, then set the repo on the Playwright tab.");
  expect(screen.queryByRole("button", { name: "Open the Playwright tab" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Open Modules" }));
  expect(onOpenModules).toHaveBeenCalledTimes(1);
  expect(onOpenPlaywright).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("shows once per app start", async () => {
  setModulesForTest(["playwright"]);
  answer(false);
  const first = mount();
  await screen.findByRole("dialog", { name: "Playwright is needed" });
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  first.view.unmount();
  // Auto Run opened again in the same app start: asked again, but silent.
  mount();
  await waitFor(() => expect(screen.getByText("nothing to say")).toBeInTheDocument());
  await act(async () => {
    await new Promise((r) => setTimeout(r, 30));
  });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("asks again each time Auto Run opens", async () => {
  setModulesForTest(["playwright"]);
  let asked = 0;
  mockIPC((cmd) => {
    if (cmd === "pw_rules_status") {
      asked += 1;
      return { clone_set: true };
    }
    return undefined;
  });
  const first = mount();
  await waitFor(() => expect(asked).toBe(1));
  first.view.unmount();
  mount();
  await waitFor(() => expect(asked).toBe(2));
});

test("does not show when a repo is set", async () => {
  setModulesForTest(["playwright"]);
  answer(true);
  mount();
  await act(async () => {
    await new Promise((r) => setTimeout(r, 30));
  });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

for (const on of [true, false]) {
  test(`is accessible with the Playwright module ${on ? "on" : "off"}`, async () => {
    setModulesForTest(on ? ["playwright"] : []);
    answer(false);
    mount();
    await screen.findByRole("dialog", { name: "Playwright is needed" });
    const results = await axe.run(document.body, { rules: { "color-contrast": { enabled: false } } });
    const problems = results.violations.map(
      (v) => `${v.id}: ${v.help} -> ${v.nodes.map((n) => n.html.slice(0, 80)).join(" | ")}`,
    );
    expect(problems).toEqual([]);
  });
}

test("with the module off, the dialog offers Open Modules; in Simple mode it offers neither the button nor its line", async () => {
  setModulesForTest([]);
  answer(false);
  const onOpenModules = vi.fn();
  mount(<Host onOpenModules={onOpenModules} />);
  await screen.findByRole("dialog", { name: "Playwright is needed" });
  expect(screen.getByText(/The Playwright module is off/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Open Modules" })).toBeInTheDocument();
  cleanup();

  resetPlaywrightNoticeForTests();
  setAppSimpleModeForTest(true);
  mount(<Host onOpenModules={onOpenModules} />);
  await screen.findByRole("dialog", { name: "Playwright is needed" });
  expect(screen.queryByText(/The Playwright module is off/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Open Modules" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Close" })).toBeInTheDocument();
});

test("in Simple mode the Playwright tab is hidden, so the dialog does not offer to open it", async () => {
  setModulesForTest(["playwright"]);
  setAppSimpleModeForTest(true);
  answer(false);
  const onOpenPlaywright = vi.fn();
  mount(<Host onOpenPlaywright={onOpenPlaywright} />);
  await screen.findByRole("dialog", { name: "Playwright is needed" });
  expect(screen.queryByRole("button", { name: "Open the Playwright tab" })).not.toBeInTheDocument();
  expect(screen.getByText("Turn Simple mode off in Settings to set one.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Close" })).toBeInTheDocument();
});

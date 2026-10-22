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
import { resetModulesStore, setModulesForTest, useModule } from "../../lib/modules";
import PlaywrightNeededDialog, { resetPlaywrightNoticeForTests, usePlaywrightNotice } from "./PlaywrightNeededDialog";

beforeEach(() => {
  resetPlaywrightNoticeForTests();
  resetModulesStore();
});
afterEach(() => {
  cleanup();
  clearMocks();
  resetModulesStore();
});

function answer(clone_set: boolean) {
  mockIPC((cmd) => (cmd === "pw_rules_status" ? { clone_set } : undefined));
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

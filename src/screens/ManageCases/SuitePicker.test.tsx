import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import type { SuiteRef } from "../../bindings";
import SuitePicker, { type PickedSuite } from "./SuitePicker";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const PLANS = [
  {
    plan: { id: 9, name: "Auth - Test Plan", area_path: "Proj\\Auth", root_suite_id: 90 },
    suites: [
      { id: 91, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
      { id: 92, name: "Smoke", suite_type: "staticTestSuite", requirement_id: null, parent_id: 91 },
      { id: 93, name: "PBI 42 suite", suite_type: "requirementTestSuite", requirement_id: 42, parent_id: null },
    ],
  },
  {
    plan: { id: 10, name: "Billing - Test Plan", area_path: "Proj\\Billing", root_suite_id: 100 },
    suites: [
      { id: 101, name: "Invoices", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
    ],
  },
];

function Harness({ onPick }: { onPick: (p: PickedSuite | null) => void }) {
  const [picked, setPicked] = useState<PickedSuite | null>(null);
  return (
    <SuitePicker
      org="acme"
      project="Web"
      picked={picked}
      onPick={(p) => {
        setPicked(p);
        onPick(p);
      }}
    />
  );
}

function renderPicker(onPick: (p: PickedSuite | null) => void = () => {}) {
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_plans_with_suites") return PLANS;
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Harness onPick={onPick} />
    </QueryClientProvider>,
  );
}

test("picking a plan lists its suites indented by depth; picking a suite reports it", async () => {
  const picks: Array<PickedSuite | null> = [];
  renderPicker((p) => picks.push(p));

  const plan = await screen.findByRole("combobox", { name: "Test plan" });
  expect(plan).toHaveTextContent("Loading plans");
  // The Select's own options only exist once open, and only once the plan
  // list has loaded (the trigger is disabled until then) - unlike a native
  // <select>, which always renders its <option>s up front.
  await waitFor(() => expect(plan).toBeEnabled());
  fireEvent.click(plan);
  fireEvent.click(await screen.findByRole("option", { name: "Auth - Test Plan" }));

  const suite = screen.getByRole("combobox", { name: "Test suite" });
  await waitFor(() => expect(suite).toBeEnabled());
  fireEvent.click(suite);
  const labels = screen.getAllByRole("option").map((o) => o.textContent);
  // Depth-first, the child indented under its parent, PBI suites tagged.
  expect(labels).toEqual(["Pick a suite", "Regression", "    Smoke", "PBI 42: PBI 42 suite"]);

  // The accessible-name algorithm collapses the leading non-breaking
  // spaces the way it collapses ordinary whitespace; textContent (checked
  // above) does not.
  fireEvent.click(screen.getByRole("option", { name: "Smoke" }));
  const lastPick = picks[picks.length - 1];
  expect(lastPick).toMatchObject({
    planId: 9,
    planName: "Auth - Test Plan",
    rootSuiteId: 90,
    suite: { id: 92, name: "Smoke" },
  });
  expect(lastPick!.siblings.map((s: SuiteRef) => s.id)).toEqual([91, 92, 93]);
});

test("changing the plan clears the picked suite", async () => {
  const picks: Array<PickedSuite | null> = [];
  renderPicker((p) => picks.push(p));
  const plan = await screen.findByRole("combobox", { name: "Test plan" });
  await waitFor(() => expect(plan).toBeEnabled());
  fireEvent.click(plan);
  fireEvent.click(await screen.findByRole("option", { name: "Auth - Test Plan" }));
  const suite = screen.getByRole("combobox", { name: "Test suite" });
  await waitFor(() => expect(suite).toBeEnabled());
  fireEvent.click(suite);
  fireEvent.click(screen.getByRole("option", { name: "Regression" }));
  expect(picks[picks.length - 1]).toMatchObject({ planId: 9, suite: { id: 91 } });
  fireEvent.click(plan);
  fireEvent.click(screen.getByRole("option", { name: "Billing - Test Plan" }));
  expect(picks[picks.length - 1]).toBeNull();
  expect(screen.getByRole("combobox", { name: "Test suite" })).toHaveTextContent("Pick a suite");
});

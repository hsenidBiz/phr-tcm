// The Setup panel's Fixtures row: shown only while the API Templates module
// is on, it says how many fixtures there are and how they last went, and its
// Open button takes the person to the API Templates screen's Fixtures tab.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { askForFixturesView } from "../../lib/apiTemplates";
import { resetModulesStore, setModulesForTest } from "../../lib/modules";
import ApiTemplates from "../ApiTemplates";
import AutoRun from "./index";

beforeEach(() => {
  setModulesForTest(["db_access", "auto_run", "api_templates"]);
});

afterEach(() => {
  resetModulesStore();
  clearMocks();
  localStorage.clear();
});

const PBI = { id: 42, title: "Login flow", work_item_type: "Product Backlog Item", project: "Web" };

/** A saved-file timestamp that long ago, the way Rust writes it (UTC). */
function stamp(hoursAgo: number): string {
  return new Date(Date.now() - hoursAgo * 3_600_000).toISOString().slice(0, 19).replace("T", " ");
}

function fixture(id: string, runs: { at: string; ok: boolean }[]) {
  return {
    fixture: { id, name: id, account: "a", steps: [], outputs: {}, creates: [] },
    runs,
  };
}

function mount(saved: ReturnType<typeof fixture>[], onOpenFixtures = vi.fn()) {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [];
    if (cmd === "env_list") {
      return {
        active: "qa",
        environments: [
          {
            id: "qa",
            name: "QA",
            start_url: "https://qa.example.com/",
            allowed_origins: [],
            db_id: null,
            test_environment: false,
            has_default_password: false,
          },
        ],
      };
    }
    if (cmd === "auto_run_list_accounts") return [{ key: "a", label: "A", username: "u", password: "p" }];
    if (cmd === "api_fixtures_list") return saved;
    if (cmd === "auto_run_list_runs") return [];
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="Web" pbi={PBI} onOpenFixtures={onOpenFixtures} />
    </QueryClientProvider>,
  );
  return onOpenFixtures;
}

const panel = () => screen.getByRole("region", { name: "Setup" });
const summary = () => within(panel()).getByRole("list", { name: "Setup summary" });

test("the Fixtures row is absent while the API Templates module is off", async () => {
  setModulesForTest(["db_access", "auto_run"]);
  mount([fixture("a", [])]);
  await waitFor(() => expect(within(summary()).getByText("Accounts")).toBeInTheDocument());
  expect(within(summary()).queryByText("Fixtures")).not.toBeInTheDocument();
  fireEvent.click(within(panel()).getByRole("button", { name: "Show setup details" }));
  expect(within(panel()).queryByRole("group", { name: "Fixtures" })).not.toBeInTheDocument();
});

test("with no fixtures the row says so", async () => {
  mount([]);
  expect(await within(summary()).findByText("No fixtures yet")).toBeInTheDocument();
  expect(within(panel()).queryByText("Needs attention")).not.toBeInTheDocument();
});

test("with fixtures the row gives the count and the last run", async () => {
  mount([fixture("a", [{ at: stamp(2), ok: true }]), fixture("b", [{ at: stamp(30), ok: true }]), fixture("c", [])]);
  expect(await within(summary()).findByText("3 fixtures, last run 2 hours ago")).toBeInTheDocument();
  expect(within(panel()).queryByText("Needs attention")).not.toBeInTheDocument();
});

test("a fixture whose last run failed makes the row need a look and the panel need attention", async () => {
  mount([
    fixture("a", [{ at: stamp(2), ok: true }]),
    // Newest first: the last run failed, though an earlier one passed.
    fixture("b", [
      { at: stamp(1), ok: false },
      { at: stamp(5), ok: true },
    ]),
  ]);
  const line = (await within(summary()).findByText("2 fixtures, 1 failed on its last run")).closest("li")!;
  expect(line.querySelector("[data-status-dot]")).toHaveAttribute("data-status-dot", "warning");
  expect(line).toHaveTextContent(", needs a look");
  expect(within(panel()).getByText("Needs attention")).toBeInTheDocument();
});

test("Open takes the person to the Fixtures", async () => {
  const onOpenFixtures = mount([fixture("a", [{ at: stamp(2), ok: true }])]);
  await within(summary()).findByText(/1 fixture, last run/);
  fireEvent.click(within(panel()).getByRole("button", { name: "Show setup details" }));
  fireEvent.click(within(panel()).getByRole("button", { name: "Open fixtures" }));
  expect(onOpenFixtures).toHaveBeenCalledTimes(1);
});

test("the API Templates screen opens on its Fixtures tab once Fixtures were asked for", async () => {
  mockIPC((cmd) => {
    if (cmd === "api_templates_overview") return { origin: null, templates: [], flows: [] };
    if (cmd === "api_fixtures_list") return [fixture("a", [{ at: stamp(2), ok: true }])];
    return undefined;
  });
  askForFixturesView();
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ApiTemplates org="acme" project="Web" onOpenAiBridge={() => {}} />
    </QueryClientProvider>,
  );
  const tab = await screen.findByRole("tab", { name: /^Fixtures/ });
  expect(tab).toHaveAttribute("aria-selected", "true");
});

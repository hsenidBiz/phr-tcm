// The Setup panel in Simple and in Advanced: Simple leaves out Discovery,
// Components, Fixtures and the sign-in recipe tools, unless one of them is
// what stops a run.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test } from "vitest";
import { resetModulesStore, setModulesForTest } from "../../../src/lib/modules";
import AutoRun from "../../../src/screens/AutoRun/index";

const NOTE = "Shown because a run needs it. Ask the assistant to sort it out, or switch to Advanced.";
const PBI = { id: 42, title: "Login flow", work_item_type: "Product Backlog Item", project: "Web" };

beforeEach(() => {
  setModulesForTest(["db_access", "auto_run", "api_templates"]);
});

afterEach(() => {
  resetModulesStore();
  clearMocks();
  localStorage.clear();
});

const FAILED_HERE = {
  fixture: { id: "b", name: "b", account: "a", steps: [], outputs: {}, creates: [] },
  runs: [],
  here: { state: "failed", failure: { at: "2026-11-06 10:00:00", step: null, detail: null } },
};

function mount(opts: { mode: "simple" | "advanced"; fixtures?: unknown[]; discovering?: () => boolean }) {
  mockIPC((cmd) => {
    if (cmd === "get_app_settings") return { autorun_mode: opts.mode };
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
    if (cmd === "api_fixtures_list") return opts.fixtures ?? [];
    if (cmd === "auto_run_list_runs") return [];
    if (cmd === "auto_run_discovery_active") return opts.discovering?.() ?? false;
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="Web" pbi={PBI} />
    </QueryClientProvider>,
  );
  return qc;
}

const panel = () => screen.getByRole("region", { name: "Setup" });

async function expand() {
  const toggle = await within(panel()).findByRole("button", { name: "Show setup details" });
  fireEvent.click(toggle);
  await within(panel()).findByRole("group", { name: "Accounts" });
}

const row = (name: string) => within(panel()).queryByRole("group", { name: name });

test("Simple hides Discovery, Components and Fixtures, and the sign-in recipe tools", async () => {
  mount({ mode: "simple" });
  await expand();
  await waitFor(() => expect(row("Sign-in")).toBeInTheDocument());
  expect(row("Discovery")).not.toBeInTheDocument();
  expect(row("Components")).not.toBeInTheDocument();
  expect(row("Fixtures")).not.toBeInTheDocument();
  expect(within(panel()).queryByRole("button", { name: "Record sign-in" })).not.toBeInTheDocument();
  expect(within(panel()).queryByRole("button", { name: "Edit sign-in recipe" })).not.toBeInTheDocument();
  for (const name of ["Environment", "Site address", "Accounts", "Areas", "Test files"]) {
    expect(row(name)).toBeInTheDocument();
  }
  expect(within(panel()).queryByText(NOTE)).not.toBeInTheDocument();
});

test("the collapsed summary in Simple leaves the same rows out", async () => {
  mount({ mode: "simple" });
  const summary = await within(panel()).findByRole("list", { name: "Setup summary" });
  await within(summary).findByText("Accounts");
  for (const name of ["Discovery", "Components", "Fixtures"]) {
    expect(within(summary).queryByText(name)).not.toBeInTheDocument();
  }
});

test("Advanced shows all of them, as today", async () => {
  mount({ mode: "advanced" });
  await expand();
  for (const name of ["Discovery", "Components", "Fixtures"]) {
    expect(await within(panel()).findByRole("group", { name })).toBeInTheDocument();
  }
  expect(within(panel()).getByRole("button", { name: "Record sign-in" })).toBeInTheDocument();
  expect(within(panel()).getByRole("button", { name: "Edit sign-in recipe" })).toBeInTheDocument();
  expect(within(panel()).queryByText(NOTE)).not.toBeInTheDocument();
});

test("discovery_row_shows_in_simple_only_while_a_discovery_holds_the_browser", async () => {
  let discovering = true;
  const qc = mount({ mode: "simple", discovering: () => discovering });
  await expand();
  const discovery = await within(panel()).findByRole("group", { name: "Discovery" });
  expect(within(discovery).getByRole("button", { name: /End discovery/ })).toBeInTheDocument();
  expect(within(panel()).getByText(NOTE)).toBeInTheDocument();

  discovering = false;
  await act(async () => {
    await qc.invalidateQueries({ queryKey: ["autorun-discovery-active"] });
  });
  await waitFor(() => expect(row("Discovery")).not.toBeInTheDocument());
  expect(within(panel()).queryByText(NOTE)).not.toBeInTheDocument();
});

test("a failed fixture shows the Fixtures row with the note in Simple", async () => {
  mount({ mode: "simple", fixtures: [FAILED_HERE] });
  await expand();
  expect(await within(panel()).findByRole("group", { name: "Fixtures" })).toBeInTheDocument();
  expect(within(panel()).getByText(NOTE)).toBeInTheDocument();
  expect(row("Discovery")).not.toBeInTheDocument();
  expect(row("Components")).not.toBeInTheDocument();
});

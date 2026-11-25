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

function mount(opts: {
  mode: "simple" | "advanced" | "loading";
  fixtures?: unknown[];
  discovering?: () => boolean;
  project?: string;
  recipeError?: boolean;
}) {
  mockIPC((cmd) => {
    if (cmd === "get_app_settings") {
      // "loading": the settings never answer, so the mode stays unknown.
      return opts.mode === "loading" ? new Promise(() => {}) : { autorun_mode: opts.mode };
    }
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
    if (cmd === "auto_run_load_recipe" && opts.recipeError) throw new Error("the recipe file is broken");
    if (cmd === "auto_run_discovery_active") return opts.discovering?.() ?? false;
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project={opts.project ?? "Web"} scope={{ kind: "pbi", pbi: PBI }} />
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

test("a failed fixture stays hidden in Simple: no Fixtures row, no header reason", async () => {
  mount({ mode: "simple", fixtures: [FAILED_HERE] });
  await expand();
  expect(row("Fixtures")).not.toBeInTheDocument();
  expect(within(panel()).queryByText(NOTE)).not.toBeInTheDocument();
  expect(panel()).not.toHaveTextContent(/fixture/i);
  expect(within(panel()).queryByText("Needs attention")).not.toBeInTheDocument();
  expect(row("Discovery")).not.toBeInTheDocument();
  expect(row("Components")).not.toBeInTheDocument();
});

test("a failed fixture still shows the Fixtures row and the header reason in Advanced", async () => {
  mount({ mode: "advanced", fixtures: [FAILED_HERE] });
  await expand();
  expect(await within(panel()).findByRole("group", { name: "Fixtures" })).toBeInTheDocument();
  expect(within(panel()).getByText("Needs attention")).toBeInTheDocument();
  expect(panel()).toHaveTextContent("1 fixture failed here - ask the assistant");
});

test("in Simple the Sign-in row says Ready, not the recipe's own words", async () => {
  mount({ mode: "simple" });
  await expand();
  const signIn = await within(panel()).findByRole("group", { name: "Sign-in" });
  await waitFor(() => expect(signIn).toHaveTextContent("Ready"));
  expect(signIn).not.toHaveTextContent("Built-in");
  expect(signIn).not.toHaveTextContent("Recipe saved");
});

test("Advanced keeps the Sign-in row's own words", async () => {
  mount({ mode: "advanced" });
  await expand();
  const signIn = await within(panel()).findByRole("group", { name: "Sign-in" });
  await waitFor(() => expect(signIn).toHaveTextContent("Built-in"));
});

test("with no project, Simple keeps the pick-a-project text, the recipe tools and the note", async () => {
  mount({ mode: "simple", project: "" });
  await expand();
  const signIn = await within(panel()).findByRole("group", { name: "Sign-in" });
  expect(signIn).toHaveTextContent("Pick a PBI, or a Home project in Settings, first");
  expect(signIn).not.toHaveTextContent("Needs setting up");
  expect(within(signIn).getByRole("button", { name: "Record sign-in" })).toBeInTheDocument();
  expect(within(signIn).getByRole("button", { name: "Edit sign-in recipe" })).toBeInTheDocument();
  expect(within(panel()).getByText(NOTE)).toBeInTheDocument();
});

test("with no project, the collapsed Simple summary says pick a project, not Needs setting up", async () => {
  mount({ mode: "simple", project: "" });
  const summary = await within(panel()).findByRole("list", { name: "Setup summary" });
  await within(summary).findByText("Sign-in");
  expect(within(summary).queryByText("Needs setting up")).not.toBeInTheDocument();
});

test("a recipe that could not be read blocks a run: Simple shows the Sign-in tools and the note", async () => {
  mount({ mode: "simple", recipeError: true });
  await expand();
  const signIn = await within(panel()).findByRole("group", { name: "Sign-in" });
  await waitFor(() => expect(signIn).toHaveTextContent("Could not be read"));
  expect(within(signIn).getByRole("button", { name: "Record sign-in" })).toBeInTheDocument();
  expect(within(signIn).getByRole("button", { name: "Edit sign-in recipe" })).toBeInTheDocument();
  expect(within(panel()).getByText(NOTE)).toBeInTheDocument();
});

test("Simple offers Release Auto Test browser with no discovery running; Advanced is unchanged", async () => {
  mount({ mode: "simple" });
  await expand();
  expect(row("Discovery")).not.toBeInTheDocument();
  expect(await within(panel()).findByRole("button", { name: "Release Auto Test browser" })).toBeInTheDocument();
});

test("Advanced offers Release Auto Test browser once, in the Discovery row", async () => {
  mount({ mode: "advanced" });
  await expand();
  const discovery = await within(panel()).findByRole("group", { name: "Discovery" });
  expect(within(discovery).getByRole("button", { name: "Release Auto Test browser" })).toBeInTheDocument();
  expect(within(panel()).getAllByRole("button", { name: "Release Auto Test browser" })).toHaveLength(1);
});

test("Simple during a discovery shows Release Auto Test browser once, in the Discovery row", async () => {
  mount({ mode: "simple", discovering: () => true });
  await expand();
  await within(panel()).findByRole("group", { name: "Discovery" });
  expect(within(panel()).getAllByRole("button", { name: "Release Auto Test browser" })).toHaveLength(1);
});

test("the panel renders as Advanced while the mode is still loading", async () => {
  mount({ mode: "loading" });
  await expand();
  for (const name of ["Discovery", "Components", "Fixtures"]) {
    expect(await within(panel()).findByRole("group", { name })).toBeInTheDocument();
  }
  expect(within(panel()).getByRole("button", { name: "Record sign-in" })).toBeInTheDocument();
  expect(within(panel()).queryByText(NOTE)).not.toBeInTheDocument();
});

test("the collapsed summary puts the blocker note in its own block under the line", async () => {
  mount({ mode: "simple", discovering: () => true });
  const summary = await within(panel()).findByRole("list", { name: "Setup summary" });
  const note = await within(summary).findByText(NOTE);
  const item = note.closest("li")!;
  expect(within(item).getByText("Discovery")).toBeInTheDocument();
  // The note is a sibling block of the line's own flex row, never a flex item
  // inside it, so the label and value keep their line whole.
  const line = within(item).getByText("Discovery").closest("div")!;
  expect(line.parentElement).toBe(item);
  expect(line).not.toContainElement(note);
  expect(note.parentElement).toBe(item);
  expect(note.tagName).toBe("P");
});

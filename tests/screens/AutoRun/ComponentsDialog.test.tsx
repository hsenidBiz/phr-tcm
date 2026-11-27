// The Components dialog: the project's saved components grouped by the
// area each was tried in, one line each (cases, version, Unused); a row
// opens to its inputs, history and cases. Remove is held while a script
// uses one and asks first otherwise; a file that cannot be read offers Reset.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { ComponentView } from "../../../src/bindings";
import ComponentsDialog, { componentsSummary } from "../../../src/screens/AutoRun/ComponentsDialog";

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const TRIED = Date.UTC(2026, 9, 1, 12);

function component(over: Partial<ComponentView> & { name: string }): ComponentView {
  return {
    description: "",
    inputs: [],
    tried_area: "Leave",
    tried_at: TRIED,
    version: 1,
    changes: 0,
    used_by_cases: [],
    ...over,
  };
}

type Call = { cmd: string; args: Record<string, unknown> };

/** Components that answer like Rust's: remove takes one out. */
function mount(components: ComponentView[]) {
  const list = [...components];
  const calls: Call[] = [];
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    if (cmd === "auto_run_load_components") return { components: list.map((c) => ({ ...c })) };
    if (cmd === "auto_run_remove_component") {
      const i = list.findIndex((c) => c.name === args.name);
      const [gone] = list.splice(i, 1);
      return gone.name;
    }
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ComponentsDialog org="acme" project="Web" onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  return { calls };
}

const open = (name: string) => fireEvent.click(screen.getByRole("button", { name: `Show ${name}` }));

test("groups_by_area_with_counts_and_other_last", async () => {
  mount([
    component({ name: "close-toast", tried_area: "", tried_at: null }),
    component({ name: "pick-date", tried_area: "Leave" }),
    component({ name: "pick-month", tried_area: "Leave" }),
    component({ name: "save-form", tried_area: "Payroll" }),
  ]);
  await screen.findByRole("listitem", { name: "pick-date" });
  const headings = screen.getAllByRole("heading", { level: 3 }).map((h) => h.textContent);
  // First appearance orders the groups; a component with no area is Other, last.
  expect(headings).toEqual(["Leave· 2", "Payroll· 1", "Other· 1"]);
  expect(screen.getByText("4 components · used by 0 cases")).toBeInTheDocument();
});

test("the summary counts distinct cases", async () => {
  mount([component({ name: "a", used_by_cases: [1, 2] }), component({ name: "b", used_by_cases: [2, 3] })]);
  expect(await screen.findByText("2 components · used by 3 cases")).toBeInTheDocument();
});

test("a row shows its cases, version and Unused", async () => {
  mount([
    component({ name: "pick-date", version: 4, used_by_cases: [4, 12] }),
    component({ name: "one", used_by_cases: [7] }),
    component({ name: "unused" }),
  ]);
  const pick = await screen.findByRole("listitem", { name: "pick-date" });
  expect(within(pick).getByText("2 cases")).toBeInTheDocument();
  expect(within(pick).getByText("v4")).toBeInTheDocument();
  expect(within(pick).queryByText("Unused")).not.toBeInTheDocument();
  expect(within(screen.getByRole("listitem", { name: "one" })).getByText("1 case")).toBeInTheDocument();
  expect(within(screen.getByRole("listitem", { name: "unused" })).getByText("Unused")).toBeInTheDocument();
  // Collapsed: the detail is not rendered until a row opens.
  expect(screen.queryByText(/never changed/)).not.toBeInTheDocument();
});

test("opening a component shows its detail, five cases first and the rest on request", async () => {
  mount([
    component({
      name: "pick-date",
      description: "Picks a day in the date picker",
      inputs: [
        { name: "field", kind: "target", description: "the date field" },
        { name: "day", kind: "text", description: "the day to pick" },
      ],
      version: 4,
      changes: 3,
      used_by_cases: [1, 2, 3, 4, 5, 6, 7],
    }),
    component({ name: "close-toast", tried_at: null, tried_area: "", changes: 0 }),
  ]);
  await screen.findByRole("listitem", { name: "pick-date" });
  const toggle = screen.getByRole("button", { name: "Show pick-date" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(toggle);
  const pick = screen.getByRole("listitem", { name: "pick-date" });
  expect(screen.getByRole("button", { name: "Hide pick-date" })).toHaveAttribute("aria-expanded", "true");
  expect(within(pick).getByText("Picks a day in the date picker")).toBeInTheDocument();
  const inputs = within(pick).getByRole("list", { name: "Inputs of pick-date" });
  expect(within(inputs).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    "field · target · the date field",
    "day · text · the day to pick",
  ]);
  const date = new Date(TRIED).toLocaleDateString();
  expect(within(pick).getByText(`Tried ${date} · changed 3 times`)).toBeInTheDocument();
  for (const id of ["1", "2", "3", "4", "5"]) expect(within(pick).getByText(id)).toBeInTheDocument();
  expect(within(pick).queryByText("6")).not.toBeInTheDocument();
  fireEvent.click(within(pick).getByRole("button", { name: "and 2 more" }));
  expect(within(pick).getByText("6")).toBeInTheDocument();
  expect(within(pick).getByText("7")).toBeInTheDocument();
  expect(within(pick).queryByRole("button", { name: /more/ })).not.toBeInTheDocument();

  open("close-toast");
  expect(screen.getByText("Not tried yet · never changed")).toBeInTheDocument();
});

test("search filters by name, description, input name and case id, and says when nothing matches", async () => {
  mount([
    component({ name: "pick-date", description: "Picks a day", used_by_cases: [412] }),
    component({ name: "close-toast", inputs: [{ name: "selector", kind: "target", description: "" }] }),
  ]);
  await screen.findByRole("listitem", { name: "pick-date" });
  const box = screen.getByRole("textbox", { name: "Search components" });
  const shown = () => screen.queryAllByRole("listitem").map((li) => li.getAttribute("aria-label"));
  fireEvent.change(box, { target: { value: "toast" } });
  expect(shown()).toContain("close-toast");
  expect(shown()).not.toContain("pick-date");
  fireEvent.change(box, { target: { value: "a day" } });
  expect(shown()).toContain("pick-date");
  expect(shown()).not.toContain("close-toast");
  fireEvent.change(box, { target: { value: "selector" } });
  expect(shown()).toContain("close-toast");
  fireEvent.change(box, { target: { value: "412" } });
  expect(shown()).toContain("pick-date");
  expect(shown()).not.toContain("close-toast");
  fireEvent.change(box, { target: { value: "zzz" } });
  expect(screen.getByText("No components match \u201czzz\u201d.")).toBeInTheDocument();
});

test("Expand all and Collapse all open and fold every row", async () => {
  mount([component({ name: "a", description: "first" }), component({ name: "b", description: "second" })]);
  await screen.findByRole("listitem", { name: "a" });
  fireEvent.click(screen.getByRole("button", { name: "Expand all" }));
  expect(screen.getByText("first")).toBeInTheDocument();
  expect(screen.getByText("second")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Collapse all" }));
  expect(screen.queryByText("first")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Expand all" })).toBeInTheDocument();
});

test("an empty file says none are saved yet", async () => {
  mount([]);
  expect(await screen.findByText(/None yet/)).toBeInTheDocument();
  expect(screen.queryByRole("list", { name: "Saved components" })).not.toBeInTheDocument();
  expect(screen.queryByRole("textbox", { name: "Search components" })).not.toBeInTheDocument();
});

test("remove_is_disabled_while_in_use_and_confirms_otherwise", async () => {
  const { calls } = mount([component({ name: "pick-date", used_by_cases: [4, 12] }), component({ name: "unused" })]);
  await screen.findByRole("listitem", { name: "pick-date" });
  fireEvent.click(screen.getByRole("button", { name: "Expand all" }));

  // Held while a script uses it; the reason is the title.
  const held = screen.getByRole("button", { name: "Remove pick-date" });
  expect(held).toBeDisabled();
  expect(held).toHaveAttribute("title", "Scripts still use it");

  const free = screen.getByRole("button", { name: "Remove unused" });
  expect(free).toBeEnabled();
  fireEvent.click(free);
  const ask = screen.getByRole("group", { name: "Remove unused?" });
  expect(ask).toHaveTextContent("Remove unused? Scripts no longer use it.");
  expect(calls.some((c) => c.cmd === "auto_run_remove_component")).toBe(false);

  // Keep backs out and calls nothing.
  fireEvent.click(within(ask).getByRole("button", { name: "Keep" }));
  expect(screen.queryByRole("group", { name: "Remove unused?" })).not.toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "auto_run_remove_component")).toBe(false);

  fireEvent.click(screen.getByRole("button", { name: "Remove unused" }));
  fireEvent.click(within(screen.getByRole("group", { name: "Remove unused?" })).getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(screen.queryByRole("listitem", { name: "unused" })).not.toBeInTheDocument());
  expect(calls.find((c) => c.cmd === "auto_run_remove_component")?.args).toEqual({
    organization: "acme",
    project: "Web",
    name: "unused",
  });
  expect(screen.getByRole("listitem", { name: "pick-date" })).toBeInTheDocument();
});

test("a refused remove says why", async () => {
  mockIPC((cmd) => {
    if (cmd === "auto_run_load_components") return { components: [component({ name: "unused" })] };
    if (cmd === "auto_run_remove_component") throw "unused is used by case 9: change that script first.";
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ComponentsDialog org="acme" project="Web" onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  // Remove now lives in the opened row, so the row opens first.
  fireEvent.click(await screen.findByRole("button", { name: "Show unused" }));
  fireEvent.click(screen.getByRole("button", { name: "Remove unused" }));
  fireEvent.click(within(screen.getByRole("group", { name: "Remove unused?" })).getByRole("button", { name: "Remove" }));
  expect(await screen.findByText("unused is used by case 9: change that script first.")).toBeInTheDocument();
});

test("reset_shows_when_the_file_cannot_be_read", async () => {
  let damaged = true;
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "auto_run_load_components") {
      if (damaged) throw "the components file projects/acme-components.json could not be read; Reset it in Auto Test";
      return { components: [] };
    }
    if (cmd === "auto_run_reset_components") {
      damaged = false;
      return "projects/acme-components.corrupt-1.json";
    }
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ComponentsDialog org="acme" project="Web" onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  expect(await screen.findByText(/acme-components\.json could not be read/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Reset components" }));
  // No confirm: the file is kept, under the name it was moved to.
  expect(await screen.findByText("projects/acme-components.corrupt-1.json")).toBeInTheDocument();
  expect(calls).toContain("auto_run_reset_components");
  expect(await screen.findByText(/None yet/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Reset components" })).not.toBeInTheDocument();
});

test("a_file_that_reads_offers_no_reset", async () => {
  mount([component({ name: "unused" })]);
  await screen.findByRole("listitem", { name: "unused" });
  expect(screen.queryByRole("button", { name: "Reset components" })).not.toBeInTheDocument();
});

test("the summary line counts components or says none", () => {
  expect(componentsSummary(undefined)).toBeNull();
  expect(componentsSummary({ components: [] })).toBe("None yet");
  expect(componentsSummary({ components: [component({ name: "a" })] })).toBe("1 component");
  expect(componentsSummary({ components: [component({ name: "a" }), component({ name: "b" })] })).toBe(
    "2 components",
  );
});

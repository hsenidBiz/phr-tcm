// The Components card: the project's saved components, each with its
// inputs, when and where it was tried, its version and changes, and the
// scripts that use it. Remove is held while a script uses one and asks
// first otherwise; a file that cannot be read offers Reset.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { ComponentView } from "../../bindings";
import ComponentsDialog, { componentsSummary } from "./ComponentsDialog";

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
    cap: 3,
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

test("lists_components_with_inputs_users_and_changes", async () => {
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
      used_by_cases: [4, 12],
    }),
    component({ name: "close-toast", tried_at: null, tried_area: "", used_by_cases: [7] }),
    component({ name: "unused" }),
  ]);
  expect(screen.getByRole("heading", { name: "Components" })).toBeInTheDocument();
  const pick = await screen.findByRole("listitem", { name: "pick-date" });
  expect(pick).toHaveTextContent("Picks a day in the date picker");
  const inputs = within(pick).getByRole("list", { name: "Inputs of pick-date" });
  expect(within(inputs).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    "field (target): the date field",
    "day (text): the day to pick",
  ]);
  const date = new Date(TRIED).toLocaleDateString();
  expect(within(pick).getByText(`Tried ${date} in Leave`)).toBeInTheDocument();
  expect(within(pick).getByText("Version 4")).toBeInTheDocument();
  expect(within(pick).getByText("Changed 3 times; the assistant stops and asks you before changing it again")).toBeInTheDocument();
  expect(within(pick).getByText("Used by cases 4, 12")).toBeInTheDocument();
  // At the cap, an assistant can change it no more: a person has to look.
  const cap = within(pick).getByText("Needs a look");
  expect(cap.closest("[class*='bg-warning']")).not.toBeNull();

  const toast = screen.getByRole("listitem", { name: "close-toast" });
  expect(within(toast).getByText("Not tried yet")).toBeInTheDocument();
  expect(within(toast).getByText("Used by case 7")).toBeInTheDocument();
  expect(within(toast).queryByText(/Changed/)).not.toBeInTheDocument();
  expect(within(toast).queryByText("Needs a look")).not.toBeInTheDocument();
  expect(within(toast).queryByRole("list", { name: "Inputs of close-toast" })).not.toBeInTheDocument();

  expect(within(screen.getByRole("listitem", { name: "unused" })).getByText("Not used by any script")).toBeInTheDocument();
});

test("an empty file says none are saved yet", async () => {
  mount([]);
  expect(await screen.findByText(/None yet/)).toBeInTheDocument();
  expect(screen.queryByRole("list", { name: "Saved components" })).not.toBeInTheDocument();
});

test("remove_is_disabled_while_in_use_and_confirms_otherwise", async () => {
  const { calls } = mount([
    component({ name: "pick-date", used_by_cases: [4, 12] }),
    component({ name: "close-toast", used_by_cases: [7] }),
    component({ name: "unused" }),
  ]);
  await screen.findByRole("listitem", { name: "pick-date" });

  const held = screen.getByRole("button", { name: "Remove pick-date" });
  expect(held).toBeDisabled();
  expect(held).toHaveAttribute("title", "Used by cases 4, 12: change those scripts first.");
  const one = screen.getByRole("button", { name: "Remove close-toast" });
  expect(one).toBeDisabled();
  expect(one).toHaveAttribute("title", "Used by case 7: change that script first.");

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
  fireEvent.click(await screen.findByRole("button", { name: "Remove unused" }));
  fireEvent.click(within(screen.getByRole("group", { name: "Remove unused?" })).getByRole("button", { name: "Remove" }));
  expect(await screen.findByText("unused is used by case 9: change that script first.")).toBeInTheDocument();
});

test("reset_shows_when_the_file_cannot_be_read", async () => {
  let damaged = true;
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "auto_run_load_components") {
      if (damaged) throw "the components file projects/acme-components.json could not be read; Reset it in Auto Run";
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

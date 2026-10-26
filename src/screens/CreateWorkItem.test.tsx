import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import CreateWorkItem, { clearWorkItemDraft } from "./CreateWorkItem";

afterEach(() => {
  clearMocks();
  localStorage.clear();
  clearWorkItemDraft();
});

function renderScreen(allProjects = false) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <CreateWorkItem org="acme" project="Web" allProjects={allProjects} />
    </QueryClientProvider>,
  );
}

function baseMocks(onCreate?: (args: Record<string, unknown>) => void) {
  mockIPC((cmd, args) => {
    if (cmd === "list_team_members")
      return [{ display_name: "Kim Lee", unique_name: "kim@acme.com" }];
    if (cmd === "classification_paths") {
      const structure = (args as { structure: string }).structure;
      return structure === "areas" ? ["Web\\Gamma Guardians"] : ["Web\\Sprint 9"];
    }
    if (cmd === "list_project_tags") return ["smoke"];
    if (cmd === "search_pbis")
      return [{ id: 4242, title: "Login flow", work_item_type: "Product Backlog Item" }];
    if (cmd === "create_work_item") {
      onCreate?.(args as Record<string, unknown>);
      return { id: 9001, url: "https://example.invalid/wi/9001" };
    }
  });
}

test("Create stays disabled until a title is entered", async () => {
  baseMocks();
  renderScreen();
  const btn = await screen.findByRole("button", { name: "Create Task" });
  expect(btn).toBeDisabled();
  fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Wire the login flow" } });
  expect(btn).toBeEnabled();
});

test("the full form reaches create_work_item, including the parent PBI", async () => {
  // The per-project parent search; the combined one is covered by the picker's own tests.
  localStorage.setItem("tcm-v2-all-projects", "0");
  let payload: Record<string, unknown> | null = null;
  baseMocks((args) => {
    payload = args;
  });
  renderScreen();

  fireEvent.click(await screen.findByLabelText("Work item type"));
  fireEvent.click(screen.getByRole("option", { name: "Bug" }));
  fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Session timeout broken" } });
  fireEvent.click(await screen.findByLabelText("Assign to"));
  fireEvent.click(screen.getByRole("option", { name: "Kim Lee" }));
  fireEvent.click(screen.getByLabelText("Priority"));
  fireEvent.click(screen.getByRole("option", { name: "1" }));
  fireEvent.change(screen.getByLabelText("Description"), { target: { value: "Repro: idle 30min" } });

  // Parent PBI via the search picker - no Enter: the search fires as you
  // type (debounced), which is the behavior under test here too.
  const find = screen.getByLabelText("Find PBI");
  fireEvent.change(find, { target: { value: "Login" } });
  fireEvent.click(await screen.findByText(/Login flow/));

  fireEvent.click(screen.getByRole("button", { name: "Create Bug" }));
  await waitFor(() => expect(payload).not.toBeNull());
  const item = (payload as unknown as { item: Record<string, unknown> }).item;
  expect(item.wi_type).toBe("Bug");
  expect(item.title).toBe("Session timeout broken");
  expect(item.assigned_to).toBe("kim@acme.com");
  expect(item.priority).toBe(1);
  expect(item.description).toBe("Repro: idle 30min");
  expect(item.parent_id).toBe(4242);

  // Success panel with the created id; Create another clears the title only.
  expect(await screen.findByText("#9001")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Create another" }));
  expect(screen.getByLabelText("Title")).toHaveValue("");
  expect(screen.getByLabelText("Work item type")).toHaveTextContent("Bug"); // context kept
});

/// Switching sections remounts every screen - that is what animates them
/// in - so a half-written item must live PAST the component. A trip to
/// the Board and back should read like the user never left.
test("a half-written form survives leaving the screen and coming back", async () => {
  baseMocks();
  const first = renderScreen();
  fireEvent.change(await screen.findByLabelText("Title"), {
    target: { value: "Half-written task" },
  });
  fireEvent.change(screen.getByLabelText("Description"), { target: { value: "Notes so far" } });
  first.unmount();

  renderScreen();
  expect(await screen.findByLabelText("Title")).toHaveValue("Half-written task");
  expect(screen.getByLabelText("Description")).toHaveValue("Notes so far");
});

/// The flip side: once the item exists, the draft must NOT keep the
/// submitted content - a form still holding it would invite creating
/// the same item twice from a later visit.
test("a created item's content does not linger in the draft", async () => {
  baseMocks();
  const first = renderScreen();
  fireEvent.change(await screen.findByLabelText("Title"), { target: { value: "Ship it" } });
  fireEvent.click(screen.getByRole("button", { name: "Create Task" }));
  await screen.findByText("#9001");
  first.unmount();

  renderScreen();
  expect(await screen.findByLabelText("Title")).toHaveValue("");
});

/// Where a new item lands. While the projects show together the screen's own
/// project is the home project, and the parent decides: an item under a PBI
/// of another project is created in that project, with that project's
/// members, areas and iterations.
type Call = { cmd: string; args: Record<string, unknown> };

function combinedMocks(calls: Call[]) {
  mockIPC((cmd, args) => {
    const a = args as Record<string, unknown>;
    calls.push({ cmd, args: a });
    if (cmd === "list_team_members")
      return [{ display_name: "Kim Lee", unique_name: "kim@acme.com" }];
    if (cmd === "classification_paths") {
      const prefix = a.project as string;
      return a.structure === "areas" ? [`${prefix}\Gamma Guardians`] : [`${prefix}\Sprint 9`];
    }
    if (cmd === "list_iterations") return [];
    if (cmd === "list_project_tags") return ["smoke"];
    if (cmd === "list_projects") return [{ name: "Web" }, { name: "Payroll" }];
    if (cmd === "search_pbis_all")
      return [{ id: 4242, title: "Login flow", work_item_type: "Product Backlog Item", project: "Payroll" }];
    if (cmd === "search_pbis")
      return [{ id: 4242, title: "Login flow", work_item_type: "Product Backlog Item", project: "Payroll" }];
    if (cmd === "create_work_item") return { id: 9001, url: "https://example.invalid/wi/9001" };
  });
}

async function pickParent() {
  fireEvent.change(await screen.findByLabelText("Find PBI"), { target: { value: "Login" } });
  fireEvent.click(await screen.findByText(/Login flow/));
}

const created = (calls: Call[]) => calls.filter((c) => c.cmd === "create_work_item");

test("with a parent from another project the item is created there", async () => {
  const calls: Call[] = [];
  combinedMocks(calls);
  renderScreen(true);

  // An area picked while the item still belongs to the home project.
  fireEvent.click(await screen.findByLabelText("Area"));
  fireEvent.click(await screen.findByRole("option", { name: "Web\Gamma Guardians" }));
  expect(screen.getByLabelText("Area")).toHaveTextContent("Web\Gamma Guardians");

  await pickParent();

  // That area belongs to Web's tree; the parent moved the item to Payroll.
  await waitFor(() => expect(screen.getByLabelText("Area")).not.toHaveTextContent("Web\Gamma Guardians"));
  expect(screen.getByText(/It will be created in Payroll\./)).toBeInTheDocument();
  await waitFor(() =>
    expect(
      calls.some(
        (c) => c.cmd === "classification_paths" && c.args.project === "Payroll" && c.args.structure === "areas",
      ),
    ).toBe(true),
  );
  expect(calls.some((c) => c.cmd === "list_iterations" && c.args.project === "Payroll")).toBe(true);
  expect(calls.some((c) => c.cmd === "list_team_members" && c.args.project === "Payroll")).toBe(true);
  expect(calls.some((c) => c.cmd === "list_project_tags" && c.args.project === "Payroll")).toBe(true);

  fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Payslip rounding" } });
  fireEvent.click(screen.getByRole("button", { name: "Create Task" }));
  await waitFor(() => expect(created(calls)).toHaveLength(1));
  const sent = created(calls)[0].args as { project: string; item: Record<string, unknown> };
  expect(sent.project).toBe("Payroll");
  expect(sent.item.parent_id).toBe(4242);
  expect(sent.item.area_path).toBeNull();
  expect(sent.item.iteration_path).toBeNull();
});

test("without a parent the item goes to the home project", async () => {
  const calls: Call[] = [];
  combinedMocks(calls);
  renderScreen(true);

  fireEvent.change(await screen.findByLabelText("Title"), { target: { value: "Housekeeping" } });
  // Two projects but nothing pulling the item elsewhere: the line still names the home project.
  expect(await screen.findByText(/It will be created in Web\./)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Create Task" }));
  await waitFor(() => expect(created(calls)).toHaveLength(1));
  expect((created(calls)[0].args as { project: string }).project).toBe("Web");
});

test("clearing the parent brings the item back to the home project", async () => {
  const calls: Call[] = [];
  combinedMocks(calls);
  renderScreen(true);
  await pickParent();
  expect(await screen.findByText(/It will be created in Payroll\./)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Clear PBI" }));
  expect(await screen.findByText(/It will be created in Web\./)).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Back home" } });
  fireEvent.click(screen.getByRole("button", { name: "Create Task" }));
  await waitFor(() => expect(created(calls)).toHaveLength(1));
  expect((created(calls)[0].args as { project: string }).project).toBe("Web");
});

test("switched off the item goes to the picked project, parent or not", async () => {
  localStorage.setItem("tcm-v2-all-projects", "0");
  const calls: Call[] = [];
  combinedMocks(calls);
  renderScreen(false);

  await pickParent();
  // The line is for the projects-together mode only.
  expect(screen.queryByText(/It will be created in/)).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Per project" } });
  fireEvent.click(screen.getByRole("button", { name: "Create Task" }));
  await waitFor(() => expect(created(calls)).toHaveLength(1));
  const sent = created(calls)[0].args as { project: string; item: Record<string, unknown> };
  expect(sent.project).toBe("Web");
  expect(sent.item.parent_id).toBe(4242);
  expect(calls.some((c) => c.cmd === "classification_paths" && c.args.project === "Payroll")).toBe(false);
});

test("an organization with one project does not say where the item goes", async () => {
  let projectsAsked = false;
  mockIPC((cmd) => {
    if (cmd === "list_projects") {
      projectsAsked = true;
      return [{ name: "Web" }];
    }
    if (cmd === "list_team_members") return [];
    if (cmd === "classification_paths") return [];
    if (cmd === "list_iterations") return [];
    if (cmd === "list_project_tags") return [];
  });
  renderScreen(true);
  await screen.findByLabelText("Title");
  await waitFor(() => expect(projectsAsked).toBe(true));
  await new Promise((r) => setTimeout(r, 50));
  expect(screen.queryByText(/It will be created in/)).not.toBeInTheDocument();
});

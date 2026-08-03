import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import CreateWorkItem from "./CreateWorkItem";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderScreen() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <CreateWorkItem org="acme" project="Web" />
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

  // Parent PBI via the search picker.
  const find = screen.getByLabelText("Find PBI");
  fireEvent.change(find, { target: { value: "Login" } });
  fireEvent.keyDown(find, { key: "Enter" });
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

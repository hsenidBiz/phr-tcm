// The project's one sign-in recipe, edited as JSON: loading it back,
// catching bad JSON before anything is sent, and showing what the app
// refuses in its own words.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import RecipeEditor from "./RecipeEditor";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => { clearMocks(); vi.clearAllMocks(); });

const RECIPE = {
  start_url: "https://hr.example.internal/",
  steps: [{ kind: "click", selector: { role: "button", name: "Login" } }],
  signed_in: { css: "#m" },
  allowed_origins: [],
  session_minutes: 480,
};

function mount(
  existing: unknown,
  onSave: (args: unknown) => unknown = () => null,
  existingQuirks: unknown = [],
  onSaveQuirks: (args: unknown) => unknown = () => null,
) {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_recipe") return existing;
    if (cmd === "auto_run_save_recipe") return onSave(args);
    if (cmd === "auto_run_load_quirks") return existingQuirks;
    if (cmd === "auto_run_save_quirks") return onSaveQuirks(args);
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RecipeEditor org="acme" project="Web" onClose={onClose} />
    </QueryClientProvider>,
  );
  return onClose;
}

test("the saved recipe loads as JSON and saves back for this project", async () => {
  const calls: unknown[] = [];
  const onClose = mount(RECIPE, (a) => { calls.push(a); return null; });
  const box = (await screen.findByLabelText("Sign-in recipe JSON")) as HTMLTextAreaElement;
  await waitFor(() => expect(JSON.parse(box.value)).toEqual(RECIPE));
  fireEvent.click(screen.getByRole("button", { name: "Save recipe" }));
  await waitFor(() => expect(calls).toEqual([{ organization: "acme", project: "Web", recipe: RECIPE }]));
  expect(onClose).toHaveBeenCalled();
});

test("text that is not JSON is caught before anything is sent", async () => {
  const calls: unknown[] = [];
  mount(null, (a) => { calls.push(a); return null; });
  const box = await screen.findByLabelText("Sign-in recipe JSON");
  fireEvent.change(box, { target: { value: "{ not json" } });
  fireEvent.click(screen.getByRole("button", { name: "Save recipe" }));
  expect(await screen.findByText(/not valid JSON/)).toBeInTheDocument();
  expect(calls).toEqual([]);
});

test("what the app refuses is shown in its own words", async () => {
  mount(RECIPE, () => { throw new Error("step 1: a locator needs one of role, text or css"); });
  // Save stays blocked until the existing recipe has actually loaded (the
  // same guard ScriptEditor uses, to stop a click mid-load from writing an
  // empty recipe over one that's already there) - wait for that, the same
  // way the first test above does, before clicking.
  const box = (await screen.findByLabelText("Sign-in recipe JSON")) as HTMLTextAreaElement;
  await waitFor(() => expect(JSON.parse(box.value)).toEqual(RECIPE));
  fireEvent.click(screen.getByRole("button", { name: "Save recipe" }));
  expect(await screen.findByText(/step 1: a locator needs/)).toBeInTheDocument();
});

test("the quirks box loads existing lines", async () => {
  const quirks = [
    { text: "the grid paginates at 50 rows", by: "person", at: "1000" },
    { text: "dates render as dd/mm", by: "assistant", at: "2000" },
  ];
  mount(RECIPE, () => null, quirks);
  const box = (await screen.findByLabelText("Known quirks")) as HTMLTextAreaElement;
  await waitFor(() => expect(box.value).toBe("the grid paginates at 50 rows\ndates render as dd/mm"));
});

test("an unchanged line keeps its author and a new line is saved as the person", async () => {
  const existingQuirks = [{ text: "dates render as dd/mm", by: "assistant", at: "2000" }];
  const calls: unknown[] = [];
  mount(RECIPE, () => null, existingQuirks, (a) => { calls.push(a); return null; });
  const recipeBox = (await screen.findByLabelText("Sign-in recipe JSON")) as HTMLTextAreaElement;
  await waitFor(() => expect(JSON.parse(recipeBox.value)).toEqual(RECIPE));
  const quirksBox = (await screen.findByLabelText("Known quirks")) as HTMLTextAreaElement;
  await waitFor(() => expect(quirksBox.value).toBe("dates render as dd/mm"));
  fireEvent.change(quirksBox, { target: { value: "dates render as dd/mm\nthe grid paginates at 50 rows" } });
  fireEvent.click(screen.getByRole("button", { name: "Save recipe" }));
  await waitFor(() => expect(calls.length).toBe(1));
  const call = calls[0] as { organization: string; project: string; quirks: { text: string; by: string; at: string }[] };
  expect(call.organization).toBe("acme");
  expect(call.project).toBe("Web");
  expect(call.quirks).toEqual([
    { text: "dates render as dd/mm", by: "assistant", at: "2000" },
    { text: "the grid paginates at 50 rows", by: "person", at: expect.any(String) },
  ]);
});

test("when the recipe save is refused, the quirks are not saved", async () => {
  const quirksCalls: unknown[] = [];
  mount(
    RECIPE,
    () => { throw new Error("step 1: a locator needs one of role, text or css"); },
    [],
    (a) => { quirksCalls.push(a); return null; },
  );
  const box = (await screen.findByLabelText("Sign-in recipe JSON")) as HTMLTextAreaElement;
  await waitFor(() => expect(JSON.parse(box.value)).toEqual(RECIPE));
  fireEvent.click(screen.getByRole("button", { name: "Save recipe" }));
  expect(await screen.findByText(/step 1: a locator needs/)).toBeInTheDocument();
  expect(quirksCalls).toEqual([]);
});

test("two identical lines in the quirks box save as one", async () => {
  const calls: unknown[] = [];
  mount(RECIPE, () => null, [], (a) => { calls.push(a); return null; });
  const recipeBox = (await screen.findByLabelText("Sign-in recipe JSON")) as HTMLTextAreaElement;
  await waitFor(() => expect(JSON.parse(recipeBox.value)).toEqual(RECIPE));
  const quirksBox = await screen.findByLabelText("Known quirks");
  fireEvent.change(quirksBox, {
    target: { value: "the grid paginates at 50 rows\n  THE GRID   paginates AT 50 ROWS  " },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save recipe" }));
  await waitFor(() => expect(calls.length).toBe(1));
  const call = calls[0] as { quirks: { text: string }[] };
  expect(call.quirks).toHaveLength(1);
  expect(call.quirks[0].text).toBe("the grid paginates at 50 rows");
});

test("clearing an already-saved quirks list is still a save, not a disabled button", async () => {
  const existingQuirks = [{ text: "the grid paginates at 50 rows", by: "person", at: "1000" }];
  const quirksCalls: unknown[] = [];
  mount(null, () => null, existingQuirks, (a) => {
    quirksCalls.push(a);
    return null;
  });
  const quirksBox = (await screen.findByLabelText("Known quirks")) as HTMLTextAreaElement;
  await waitFor(() => expect(quirksBox.value).toBe("the grid paginates at 50 rows"));
  fireEvent.change(quirksBox, { target: { value: "" } });
  const button = screen.getByRole("button", { name: "Save quirks" });
  expect(button).toBeEnabled();
  fireEvent.click(button);
  await waitFor(() => expect(quirksCalls.length).toBe(1));
  const call = quirksCalls[0] as { quirks: unknown[] };
  expect(call.quirks).toEqual([]);
});

test("an empty recipe box still saves the quirks, and does not call auto_run_save_recipe", async () => {
  const recipeCalls: unknown[] = [];
  const quirksCalls: unknown[] = [];
  mount(null, (a) => { recipeCalls.push(a); return null; }, [], (a) => { quirksCalls.push(a); return null; });
  const quirksBox = await screen.findByLabelText("Known quirks");
  await waitFor(() => expect(screen.getByRole("button", { name: "Save quirks" })).toBeInTheDocument());
  fireEvent.change(quirksBox, { target: { value: "the grid paginates at 50 rows" } });
  fireEvent.click(screen.getByRole("button", { name: "Save quirks" }));
  await waitFor(() => expect(quirksCalls.length).toBe(1));
  expect(recipeCalls).toEqual([]);
  const call = quirksCalls[0] as { organization: string; project: string; quirks: { text: string }[] };
  expect(call.organization).toBe("acme");
  expect(call.project).toBe("Web");
  expect(call.quirks[0].text).toBe("the grid paginates at 50 rows");
});

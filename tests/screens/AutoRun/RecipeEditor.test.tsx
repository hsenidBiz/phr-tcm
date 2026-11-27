// The project's one sign-in recipe, edited as JSON: loading it back,
// catching bad JSON before anything is sent, and showing what the app
// refuses in its own words. The project's Known quirks are not here: they
// have their own row in Auto Test's Setup card (see QuirksDialog.test.tsx).

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import RecipeEditor from "../../../src/screens/AutoRun/RecipeEditor";

vi.mock("../../../src/lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => { clearMocks(); vi.clearAllMocks(); });

const RECIPE = {
  start_url: "https://hr.example.internal/",
  steps: [{ kind: "click", selector: { role: "button", name: "Login" } }],
  signed_in: { css: "#m" },
  allowed_origins: [],
  session_minutes: 480,
};

/** Every command the editor sent, in order. */
const sent: string[] = [];

function mount(existing: unknown, onSave: (args: unknown) => unknown = () => null) {
  sent.length = 0;
  mockIPC((cmd, raw) => {
    sent.push(cmd);
    const args = (raw ?? {}) as Record<string, unknown>;
    if (cmd === "auto_run_load_recipe") return existing;
    if (cmd === "auto_run_save_recipe") return onSave(args);
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RecipeEditor org="acme" project="Web" onClose={onClose} />
    </QueryClientProvider>,
  );
  return { onClose };
}

test("the saved recipe loads as JSON and saves back for this project", async () => {
  const calls: unknown[] = [];
  const { onClose } = mount(RECIPE, (a) => { calls.push(a); return null; });
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

// The Known quirks moved out of this dialog into their own Setup row:
// nothing of them is shown, and the editor never even reads them.
test("the recipe editor no longer shows the Known quirks", async () => {
  mount(RECIPE);
  await screen.findByLabelText("Sign-in recipe JSON");
  expect(screen.queryByRole("heading", { name: "Known quirks" })).not.toBeInTheDocument();
  expect(screen.queryByLabelText("Add a note")).not.toBeInTheDocument();
  expect(sent).not.toContain("auto_run_load_quirks");
  // What is left is the recipe, with its word defined where it first shows.
  const recipe = screen.getByRole("heading", { name: "Recipe", level: 3 });
  expect(recipe.closest("section")).toContainElement(screen.getByRole("button", { name: "Save recipe" }));
  expect(screen.getByRole("heading", { name: "Sign-in recipe", level: 2 })).toContainElement(
    screen.getByText("recipe", { selector: "[data-term]" }),
  );
});

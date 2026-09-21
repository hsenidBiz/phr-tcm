// The project's one sign-in recipe, edited as JSON: loading it back,
// catching bad JSON before anything is sent, and showing what the app
// refuses in its own words.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import RecipeEditor from "./RecipeEditor";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => { clearMocks(); vi.clearAllMocks(); });

const RECIPE = {
  start_url: "https://hr.example.internal/",
  steps: [{ kind: "click", selector: { role: "button", name: "Login" } }],
  signed_in: { css: "#m" },
  allowed_origins: [],
  session_minutes: 480,
};

function mount(existing: unknown, onSave: (args: unknown) => unknown = () => null) {
  mockIPC((cmd, args) => {
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

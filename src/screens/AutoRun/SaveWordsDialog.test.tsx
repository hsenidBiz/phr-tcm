// The project's own save words: the built-in ones fixed, the project's
// added and removed, and the whole list saved through one command.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { NavView } from "../../bindings";
import { toast } from "../../lib/toast";
import SaveWordsDialog from "./SaveWordsDialog";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const BUILT_IN = ["save", "update", "delete", "submit", "approve", "publish", "assign"];

function view(save_words: string[]): NavView {
  return { direct_urls: true, modules: [], save_words, built_in_save_words: BUILT_IN };
}

/** Mounts the dialog; every save it sends lands in `sent`, answered by
 * `answer` (a list it would keep) or refused with `refuse`. */
function mount(start: string[], sent: string[][], refuse?: string) {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_set_save_words") {
      const words = (args as { words: string[] }).words;
      sent.push(words);
      if (refuse) throw refuse;
      return view(words);
    }
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <SaveWordsDialog org="acme" project="PMS" view={view(start)} onClose={onClose} />
    </QueryClientProvider>,
  );
  return onClose;
}

test("the built-in words are shown and cannot be removed", () => {
  mount([], []);
  const fixed = screen.getByRole("list", { name: "Built-in save words" });
  for (const w of BUILT_IN) expect(within(fixed).getByText(w)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Remove save word/ })).not.toBeInTheDocument();
  expect(screen.getByText("None yet.")).toBeInTheDocument();
});

test("a word is added, trimmed and lowercased, and the whole list is saved", async () => {
  const sent: string[][] = [];
  const onClose = mount(["recalc"], sent);
  fireEvent.change(screen.getByRole("textbox", { name: "New save word" }), { target: { value: "  Search " } });
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  expect(within(screen.getByRole("list", { name: "This project's save words" })).getByText("search")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(sent).toEqual([["recalc", "search"]]));
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(toast.success).toHaveBeenCalledWith("Save words saved.");
});

test("a word is removed and the list saved without it", async () => {
  const sent: string[][] = [];
  mount(["recalc", "search"], sent);
  fireEvent.click(screen.getByRole("button", { name: "Remove save word recalc" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(sent).toEqual([["search"]]));
});

test("a built-in word is not added twice", () => {
  const sent: string[][] = [];
  mount([], sent);
  fireEvent.change(screen.getByRole("textbox", { name: "New save word" }), { target: { value: "Save" } });
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  expect(screen.getByText("\"save\" is already a built-in save word")).toBeInTheDocument();
  expect(screen.getByText("None yet.")).toBeInTheDocument();
});

test("a refusal from the app is shown and the dialog stays open", async () => {
  const sent: string[][] = [];
  const onClose = mount(["x?y"], sent, "\"x?y\" cannot match - a save word is matched against the path, never the query or a fragment");
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByText(/cannot match - a save word is matched against the path/)).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
});

test("the dialog is named by its heading", () => {
  mount([], []);
  expect(screen.getByRole("dialog", { name: "Save words" })).toBeInTheDocument();
});

test("a word typed but not yet added is added when Save is pressed", async () => {
  const sent: string[][] = [];
  mount(["recalc"], sent);
  fireEvent.change(screen.getByRole("textbox", { name: "New save word" }), { target: { value: " Approve2 " } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(sent).toEqual([["recalc", "approve2"]]));
});

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import CommandPalette from "./CommandPalette";
import { SHORTCUT_ORDER, VISIBLE_CASE_ITEMS, sectionShortcut } from "./Sidebar";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderPalette(onNavigate = vi.fn(), onSwitchProject = vi.fn(), onToggleWork = vi.fn()) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <CommandPalette
        onNavigate={onNavigate}
        org="acme"
        onSwitchProject={onSwitchProject}
        onToggleWork={onToggleWork}
      />
    </QueryClientProvider>,
  );
  return { onNavigate, onSwitchProject, onToggleWork };
}

test("Ctrl+K opens the palette and navigation fires", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_projects") return [{ id: "p1", name: "Web" }];
  });
  const { onNavigate, onToggleWork } = renderPalette();

  fireEvent.keyDown(window, { key: "k", ctrlKey: true });
  expect(await screen.findByPlaceholderText(/Type a command/)).toBeInTheDocument();

  fireEvent.click(await screen.findByText("Run Tests"));
  expect(onNavigate).toHaveBeenCalledWith("run");

  fireEvent.keyDown(window, { key: "k", ctrlKey: true });
  fireEvent.click(await screen.findByText("Toggle Work Manager"));
  expect(onToggleWork).toHaveBeenCalled();
});

test("switch-project entries come from the current org", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_projects") return [{ id: "p1", name: "Web" }];
  });
  const { onSwitchProject } = renderPalette();
  fireEvent.keyDown(window, { key: "k", ctrlKey: true });
  fireEvent.click(await screen.findByText("Web"));
  expect(onSwitchProject).toHaveBeenCalledWith("Web");
});

// The hints were once literals and drifted the moment a tab was added:
// "mod+4" said Run Tests while Ctrl+4 opened View Test Cases. Each row's
// digit must be its 1-based slot in the same order App's Ctrl+N uses.
test("every Go-to hint is the section's 1-based slot in the shortcut order", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_projects") return [];
  });
  renderPalette();
  fireEvent.keyDown(window, { key: "k", ctrlKey: true });
  await screen.findByPlaceholderText(/Type a command/);

  expect(VISIBLE_CASE_ITEMS.length).toBeGreaterThan(0);
  for (const item of VISIBLE_CASE_ITEMS) {
    const slot = SHORTCUT_ORDER.indexOf(item.id) + 1;
    expect(sectionShortcut(item.id)).toBe(`mod+${slot}`);
    // The Kbd badge sits beside the label inside the same row, and the
    // digit is the last thing in it.
    const row = screen.getByText(item.label).closest('[data-slot="command-item"]');
    expect(row?.textContent?.trim().endsWith(String(slot))).toBe(true);
  }
  expect(sectionShortcut("settings")).toBeUndefined();
});

test("typing narrows the list; a query nothing matches says so, and Enter then runs nothing", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_projects") return [];
  });
  const { onNavigate, onToggleWork } = renderPalette();
  fireEvent.keyDown(window, { key: "k", ctrlKey: true });
  const input = await screen.findByPlaceholderText(/Type a command/);

  fireEvent.change(input, { target: { value: "sett" } });
  expect(await screen.findByText("Settings")).toBeInTheDocument();
  await waitFor(() => expect(screen.queryByText("Toggle Work Manager")).not.toBeInTheDocument());

  fireEvent.change(input, { target: { value: "zzzz" } });
  expect(await screen.findByText("No results.")).toBeInTheDocument();
  fireEvent.keyDown(input, { key: "Enter" });
  expect(onNavigate).not.toHaveBeenCalled();
  expect(onToggleWork).not.toHaveBeenCalled();
});

test("Enter runs the highlighted row - the first, as the palette opens - and closes it", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_projects") return [];
  });
  const { onNavigate } = renderPalette();
  fireEvent.keyDown(window, { key: "k", ctrlKey: true });
  const input = await screen.findByPlaceholderText(/Type a command/);
  const first = VISIBLE_CASE_ITEMS[0];
  await waitFor(() =>
    expect(screen.getByText(first.label).closest('[data-slot="command-item"]')).toHaveAttribute("data-highlighted"),
  );

  fireEvent.keyDown(input, { key: "Enter" });
  expect(onNavigate).toHaveBeenCalledWith(first.id);
  await waitFor(() => expect(screen.queryByPlaceholderText(/Type a command/)).not.toBeInTheDocument());
});

// cmdk matched a query's letters as a subsequence of a row's label, gaps
// allowed - "chk upd" found "Check for updates" without typing it out. The
// move to XiodUI's Command must keep that, not fall back to a plain
// substring search that only prefixes or exact fragments would pass.
test("a query's letters find a row as a subsequence, in order, gaps allowed", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_projects") return [];
  });
  renderPalette();
  fireEvent.keyDown(window, { key: "k", ctrlKey: true });
  const input = await screen.findByPlaceholderText(/Type a command/);

  fireEvent.change(input, { target: { value: "chk upd" } });
  expect(await screen.findByText("Check for updates")).toBeInTheDocument();

  fireEvent.change(input, { target: { value: "tgl theme" } });
  expect(await screen.findByText("Toggle theme")).toBeInTheDocument();
  await waitFor(() => expect(screen.queryByText("Check for updates")).not.toBeInTheDocument());

  // Same letters as "Toggle theme", reversed: present in the row, but not
  // in order, so no row should match.
  fireEvent.change(input, { target: { value: "emeht elggot" } });
  expect(await screen.findByText("No results.")).toBeInTheDocument();
});

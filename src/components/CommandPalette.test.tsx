import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
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
    const row = screen.getByText(item.label).closest("[cmdk-item]");
    expect(row?.textContent?.trim().endsWith(String(slot))).toBe(true);
  }
  expect(sectionShortcut("settings")).toBeUndefined();
});

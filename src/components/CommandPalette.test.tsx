import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import CommandPalette from "./CommandPalette";

afterEach(() => clearMocks());

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

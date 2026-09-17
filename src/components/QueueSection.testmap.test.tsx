import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import type { TestCase } from "../bindings";
import QueueSection from "./QueueSection";

vi.mock("../hooks/useOnScreen", () => ({ useOnScreen: () => [() => {}, true] }));

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const tc = (title: string, area?: string): TestCase => ({
  title,
  steps: [{ action: "Open page", expected: "Page shown" }],
  tags: "smoke",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
  spec_order: null,
  tester_order: null,
  ...(area ? { area } : {}),
});

function Harness({ initial }: { initial: TestCase[] }) {
  const [queue, setQueue] = useState<TestCase[]>(initial);
  return <QueueSection org="acme" project="Web" pbiId={42} queue={queue} setQueue={setQueue} />;
}

/// The queue's cases came from a file, so their area paths draw the tree.
test("Test map sends the queue as a tree of its area paths", async () => {
  let sent: { nodes: Array<{ name: string; count: number; children: Array<{ name: string }> }>; subtitle: string } | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "view_test_map_html") {
      sent = args as typeof sent;
      return null;
    }
    return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Harness
        initial={[
          tc("Grid shows columns", "Manage Events / Grid"),
          tc("Create saves", "Manage Events / Create"),
          tc("Hand-typed one"),
        ]}
      />
    </QueryClientProvider>,
  );
  await screen.findByText("Grid shows columns");

  fireEvent.click(screen.getByRole("button", { name: "Test map" }));
  await waitFor(() => expect(sent).not.toBeNull());
  expect(sent!.subtitle).toBe("PBI #42");
  expect(sent!.nodes.map((n) => n.name)).toEqual(["Manage Events", "Ungrouped"]);
  expect(sent!.nodes[0].count).toBe(2);
  expect(sent!.nodes[0].children.map((c) => c.name)).toEqual(["Create", "Grid"]);
});

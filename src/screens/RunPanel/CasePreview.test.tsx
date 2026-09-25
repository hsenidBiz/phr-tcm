import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { TestPoint } from "../../bindings";
import CasePreview from "./CasePreview";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const full = (id: number, title: string, steps: unknown[]) => ({
  id,
  title,
  steps,
  step_ids: steps.map(() => ""),
  steps_xml: "",
  tags: "",
  automation_status: "Planned",
  module_value: "",
  preconditions: "",
});

test("a Shared Steps row reads as Shared steps #N with its title, not as a blank row", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "test_cases_by_ids") {
      const ids = (args as { ids: number[] }).ids;
      if (ids[0] === 812) return [full(812, "Sign in as an admin", [])];
      return [
        full(201, "Valid login", [
          { action: "Open page", expected: "Shown" },
          { action: "", expected: "", shared: 812 },
        ]),
      ];
    }
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const point = { test_case_id: 201, last_run_id: null, last_result_id: null } as unknown as TestPoint;
  render(
    <QueryClientProvider client={qc}>
      <CasePreview org="acme" project="Web" point={point} />
    </QueryClientProvider>,
  );
  expect(await screen.findByText("Shared steps #812")).toBeInTheDocument();
  expect(await screen.findByText(/Sign in as an admin/)).toBeInTheDocument();
});

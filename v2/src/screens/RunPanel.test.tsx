import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import RunPanel from "./RunPanel";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderPanel() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <RunPanel org="acme" project="Web" pbiId={42} pbiTitle="Login flow" />
    </QueryClientProvider>,
  );
}

function mockAll(submitted: { runName?: string; outcomes?: unknown[] }) {
  mockIPC((cmd, args) => {
    switch (cmd) {
      case "plugin:event|listen":
        return 1;
      case "plugin:event|unlisten":
        return null;
      case "ensure_pbi_suite":
        return { plan_id: 9, plan_name: "Auth - Test Plan", suite_id: 91 };
      case "list_test_points":
        return [
          {
            point_id: 7,
            test_case_id: 201,
            test_case_name: "Valid login",
            config_name: "Windows 10",
            tester: "",
            last_outcome: "failed",
            last_run_id: 3,
            last_result_id: 30,
          },
          {
            point_id: 8,
            test_case_id: 202,
            test_case_name: "Invalid login",
            config_name: "Windows 10",
            tester: "",
            last_outcome: "",
            last_run_id: null,
            last_result_id: null,
          },
        ];
      case "submit_test_run": {
        const a = args as { runName: string; outcomes: unknown[] };
        submitted.runName = a.runName;
        submitted.outcomes = a.outcomes;
        return { run_id: 300, web_url: "https://x/run/300" };
      }
    }
  });
}

test("loads suite + points and records chosen outcomes", async () => {
  const submitted: { runName?: string; outcomes?: Array<{ point_id: number; outcome: string }> } = {};
  mockAll(submitted);
  renderPanel();

  expect(await screen.findByText(/Auth - Test Plan/)).toBeInTheDocument();
  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("failed")).toBeInTheDocument();

  fireEvent.change(screen.getByLabelText("Outcome for Valid login"), {
    target: { value: "Passed" },
  });
  const btn = screen.getByRole("button", { name: /Record 1 outcome/ });
  fireEvent.click(btn);

  expect(await screen.findByText("View run in Azure DevOps")).toBeInTheDocument();
  expect(submitted.runName).toBe("Login flow - manual run");
  expect(submitted.outcomes).toEqual([
    {
      point_id: 7,
      outcome: "Passed",
      comment: null,
      duration_ms: null,
      step_ids: null,
      step_outcomes: null,
      attachments: null,
      bug_ids: null,
    },
  ]);
});

test("skipped points are not submitted", async () => {
  const submitted: { outcomes?: unknown[] } = {};
  mockAll(submitted);
  renderPanel();
  await screen.findByText("Valid login");
  // Nothing chosen: button disabled.
  expect(screen.getByRole("button", { name: /Record 0 outcomes/ })).toBeDisabled();
});

test("row clicks select cases for a targeted runner session", async () => {
  mockAll({});
  renderPanel();
  await screen.findByText("Valid login");
  expect(screen.queryByRole("button", { name: /Run 1 in runner/ })).not.toBeInTheDocument();

  fireEvent.click(screen.getByText("Valid login"));
  expect(screen.getByRole("button", { name: /Run 1 in runner/ })).toBeInTheDocument();

  // Clicking again deselects.
  fireEvent.click(screen.getByText("Valid login"));
  expect(screen.queryByRole("button", { name: /Run 1 in runner/ })).not.toBeInTheDocument();
});

test("suite is resolved once, then every later mount reuses the seed", async () => {
  let ensured = 0;
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "ensure_pbi_suite") {
      ensured++;
      return { plan_id: 9, plan_name: "Fresh Plan", suite_id: 91 };
    }
    if (cmd === "list_test_points") return [];
  });

  // First visit: no seed -> one real resolve that writes the seed.
  const first = renderPanel();
  expect(await screen.findByText(/Fresh Plan/)).toBeInTheDocument();
  expect(ensured).toBe(1);
  first.unmount();

  // Tab switch back - even with a brand-new QueryClient (no in-memory
  // cache), the queryFn short-circuits to the localStorage seed.
  renderPanel();
  expect(await screen.findByText(/Fresh Plan/)).toBeInTheDocument();
  expect(ensured).toBe(1);
});

test("suite resolution is cached in localStorage and reused", async () => {
  localStorage.setItem(
    "tcm-v2-suite:acme/42",
    JSON.stringify({ plan_id: 9, plan_name: "Cached Plan", suite_id: 91 }),
  );
  let ensured = 0;
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "ensure_pbi_suite") {
      ensured++;
      return { plan_id: 9, plan_name: "Fresh Plan", suite_id: 91 };
    }
    if (cmd === "list_test_points") return [];
  });
  renderPanel();
  expect(await screen.findByText(/Cached Plan/)).toBeInTheDocument();
  expect(ensured).toBe(0);
});

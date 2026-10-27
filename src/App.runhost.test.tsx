// What App does when the unattended run's Review is pressed while the
// projects show together: a run of another organization brings that
// organization's project home; a run of this organization never moves the
// home project. The run store is stood in for, so a review can be raised
// without a run.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, configure, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import App from "./App";
import { claimCacheFor } from "./lib/cache";
import { ALL_MODULES, resetModulesStore } from "./lib/modules";
import { resetForTests as resetNotifications } from "./lib/notifications";

const store = vi.hoisted(() => ({
  review: null as unknown,
  listeners: new Set<() => void>(),
  set(next: unknown) {
    this.review = next;
    for (const l of this.listeners) l();
  },
}));

vi.mock("./lib/backgroundRun", async (orig) => {
  const actual = await orig<typeof import("./lib/backgroundRun")>();
  const { useSyncExternalStore } = await import("react");
  return {
    ...actual,
    useBackgroundRun: () => {
      const review = useSyncExternalStore(
        (cb) => {
          store.listeners.add(cb);
          return () => {
            store.listeners.delete(cb);
          };
        },
        () => store.review,
      );
      return { run: null, review };
    },
    clearReviewRequest: () => store.set(null),
  };
});

vi.setConfig({ testTimeout: 15_000 });
configure({ asyncUtilTimeout: 5_000 });

afterEach(() => {
  clearMocks();
  localStorage.clear();
  resetModulesStore();
  resetNotifications();
  store.set(null);
});

const PBI = { id: 7, title: "Pay run", work_item_type: "Product Backlog Item" };
const storedPrefs = () => JSON.parse(localStorage.getItem("tcm-v2-prefs") ?? "{}");

function renderApp() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <App />
    </QueryClientProvider>,
  );
}

function mount(savedPbi: unknown = null, extra: (cmd: string, args: unknown) => unknown = () => undefined) {
  localStorage.setItem("tcm-v2-all-projects-notice", "1");
  localStorage.setItem("tcm-v2-tour-done", "yes");
  localStorage.setItem(
    "tcm-v2-prefs",
    JSON.stringify({ org: "acme", project: "Web", section: "manual", pbi: savedPbi, workMode: false }),
  );
  mockIPC((cmd, args) => {
    if (cmd === "modules_get") return [...ALL_MODULES];
    if (cmd === "resume_session") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update") return null;
    if (cmd === "list_orgs")
      return [
        { name: "acme", url: "" },
        { name: "other", url: "" },
      ];
    if (cmd === "list_projects")
      return [
        { id: "p1", name: "Web" },
        { id: "p2", name: "Payroll" },
        { id: "p3", name: "Mobile" },
      ];
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "plugin:event|listen") return 1;
    return extra(cmd, args);
  });
  renderApp();
}

test("a run from another organization brings that run's project home", async () => {
  mount();
  await screen.findByText("a@b.com");
  await waitFor(() => expect(storedPrefs().org).toBe("acme"));

  act(() =>
    store.set({
      org: "other",
      project: "Mobile",
      pbi: { ...PBI, project: "Mobile" },
      runId: "run-1",
      toPastRuns: true,
    }),
  );

  await waitFor(() => expect(storedPrefs().org).toBe("other"));
  await waitFor(() => expect(storedPrefs().project).toBe("Mobile"));
  expect(storedPrefs().pbi?.id).toBe(7);
});

test("a run from this organization never moves the home project", async () => {
  mount();
  await screen.findByText("a@b.com");
  await waitFor(() => expect(storedPrefs().org).toBe("acme"));

  act(() =>
    store.set({
      org: "acme",
      project: "Payroll",
      pbi: { ...PBI, project: "Payroll" },
      runId: "run-1",
      toPastRuns: true,
    }),
  );

  await waitFor(() => expect(storedPrefs().pbi?.project).toBe("Payroll"));
  expect(storedPrefs().org).toBe("acme");
  expect(storedPrefs().project).toBe("Web");
});

// A project a notification handed to the Work Manager's pull requests belongs
// to the organization it was raised in; a run that takes the person to another
// organization leaves it behind. The way back in is the Work Manager button.
test("a run from another organization drops the project a notification handed over", async () => {
  claimCacheFor("a@b.com");
  localStorage.setItem(
    "tcm-v2-notifications:acme",
    JSON.stringify([
      {
        id: "pr-review:Payroll:api:7",
        kind: "pr-review",
        title: "Pull request 7 needs your review",
        body: "Fix the payslip",
        at: new Date().toISOString(),
        read: false,
        href: "https://x/pr/7",
        target: { kind: "pr", repo: "api", id: 7, project: "Payroll" },
      },
    ]),
  );
  resetNotifications();
  const asked: Array<{ organization: string; project: string }> = [];
  mount(null, (cmd, args) => {
    if (cmd === "pr_overview") {
      asked.push(args as { organization: string; project: string });
      return { awaiting: [], mine: [] };
    }
    if (cmd === "list_repos") return [];
  });
  await screen.findByText("a@b.com");
  fireEvent.click(await screen.findByRole("button", { name: /Notifications/ }));
  fireEvent.click(screen.getByRole("button", { name: "Pull request 7 needs your review" }));
  expect(await screen.findByRole("heading", { name: "Pull Requests" })).toBeInTheDocument();
  await waitFor(() => expect(asked[asked.length - 1]?.project).toBe("Payroll"));

  act(() =>
    store.set({
      org: "other",
      project: "Mobile",
      pbi: { ...PBI, project: "Mobile" },
      runId: "run-1",
      toPastRuns: true,
    }),
  );
  await waitFor(() => expect(storedPrefs().org).toBe("other"));
  await waitFor(() => expect(screen.queryByRole("heading", { name: "Pull Requests" })).not.toBeInTheDocument());

  // Back to the Work Manager from its button.
  fireEvent.click(screen.getByRole("button", { name: /Work Manager/ }));
  expect(await screen.findByRole("heading", { name: "Pull Requests" })).toBeInTheDocument();
  await waitFor(() =>
    expect(asked[asked.length - 1]).toEqual(expect.objectContaining({ organization: "other", project: "Mobile" })),
  );
});

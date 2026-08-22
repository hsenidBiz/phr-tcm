import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import ViewCases from "./ViewCases";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderView() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ViewCases
        org="acme"
        project="Web"
        pbi={{ id: 42, title: "Login flow", work_item_type: "Product Backlog Item" }}
      />
    </QueryClientProvider>,
  );
}

const caseA = {
  id: 201,
  title: "Login - valid",
  tags: "smoke",
  automation_status: "Planned",
  steps: [
    { action: "Open login page", expected: "Form shown" },
    { action: "Submit valid creds", expected: "Dashboard opens" },
  ],
  step_ids: ["2", "3"],
  module_value: "",
  preconditions: "User exists",
};
const caseB = { ...caseA, id: 202, title: "Login - locked out", tags: "" };
const caseC = { ...caseA, id: 203, title: "Checkout", tags: "" };

function mockCases(
  onView?: (queue: Array<{ update_id: number | null }>) => void,
  onRefresh?: () => void,
) {
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [caseA, caseB, caseC];
    if (cmd === "view_queue_html") {
      onView?.((args as { queue: Array<{ update_id: number | null }> }).queue);
      return null;
    }
    if (cmd === "refresh_queue_html") {
      onRefresh?.();
      return null;
    }
  });
}

/// Cached-first: flipping away and back inside a minute must render from
/// cache with zero refetch - the tab switch IS the hot path.
test("a remount within staleTime reuses the cached case list", async () => {
  let fetches = 0;
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") {
      fetches++;
      return [caseA, caseB, caseC];
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const renderWithClient = () =>
    render(
      <QueryClientProvider client={qc}>
        <ViewCases
          org="acme"
          project="Web"
          pbi={{ id: 42, title: "Login flow", work_item_type: "Product Backlog Item" }}
        />
      </QueryClientProvider>,
    );

  const view = renderWithClient(); // first mount
  await screen.findByText("Login - valid");
  expect(fetches).toBe(1);
  view.unmount();
  renderWithClient(); // same client = same cache
  await screen.findByText("Login - valid"); // renders instantly from cache
  expect(fetches).toBe(1); // FAILS before the fix (2)
});

test("rows start compact; the chevron expands steps, tags and the comment editor", async () => {
  mockCases();
  renderView();

  await screen.findByText("Login - valid");
  expect(screen.getByText("Login - valid").closest("li")!.className).toContain("cv-row");
  expect(screen.queryByText("Open login page")).not.toBeInTheDocument();
  // Tags stay out of the compact row - they only show in the detail.
  expect(screen.queryByText("smoke")).not.toBeInTheDocument();

  fireEvent.click(screen.getByLabelText("Expand #201"));
  expect(screen.getByText("Open login page")).toBeInTheDocument();
  expect(screen.getByText("Dashboard opens")).toBeInTheDocument();
  expect(screen.getByText(/User exists/)).toBeInTheDocument();
  expect(screen.getByText("smoke")).toBeInTheDocument();

  // Comment saves locally with a visible "Comment" chip on the row.
  fireEvent.click(screen.getByRole("button", { name: /Add comment/ }));
  fireEvent.change(screen.getByLabelText("Comment for #201"), {
    target: { value: "Step 2 needs the new MFA prompt" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save comment" }));
  const chip = screen.getByLabelText("Has a local comment");
  expect(chip).toHaveTextContent("Comment");
  expect(chip).toHaveAttribute("title", "Step 2 needs the new MFA prompt");

  // Clicking the chip opens a focused dialog with the comment (collapse
  // the row first so we prove the dialog is what shows it).
  fireEvent.click(screen.getByLabelText("Expand #201"));
  expect(screen.queryByText("Step 2 needs the new MFA prompt")).not.toBeInTheDocument();
  fireEvent.click(chip);
  const dialog = screen.getByRole("dialog", { name: "Comment for #201" });
  expect(dialog).toHaveTextContent("Step 2 needs the new MFA prompt");

  // Editing inside the dialog persists to the store.
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(screen.getByLabelText("Comment for #201 (edit)"), {
    target: { value: "Updated from the dialog" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save comment" }));
  expect(JSON.parse(localStorage.getItem("tcm-v2-case-notes:acme")!)["201"]).toBe(
    "Updated from the dialog",
  );

  fireEvent.click(screen.getByLabelText("Close comment"));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

  // Remove deletes the note (dialog closes, chip disappears, store empty).
  fireEvent.click(screen.getByLabelText("Has a local comment"));
  fireEvent.click(screen.getByRole("button", { name: "Remove" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.queryByLabelText("Has a local comment")).not.toBeInTheDocument();
  expect(JSON.parse(localStorage.getItem("tcm-v2-case-notes:acme")!)).toEqual({});
});

test("selection drives View in browser; nothing selected sends all visible", async () => {
  const sent: number[][] = [];
  mockCases((queue) => sent.push(queue.map((c) => c.update_id!)));
  renderView();

  // No selection: everything visible goes to the report.
  await screen.findByText("Login - valid");
  fireEvent.click(screen.getByRole("button", { name: "View All Test Cases in Browser" }));
  await waitFor(() => expect(sent).toHaveLength(1));
  expect(sent[0]).toEqual([201, 202, 203]);

  // Click + shift-click selects a range; the button reflects the count.
  fireEvent.click(screen.getByText("Login - valid"));
  fireEvent.click(screen.getByText("Login - locked out"), { shiftKey: true });
  fireEvent.click(screen.getByRole("button", { name: "View 2 Test Cases in Browser" }));
  await waitFor(() => expect(sent).toHaveLength(2));
  expect(sent[1]).toEqual([201, 202]);

  // Clicking the sole highlighted case again deselects it.
  fireEvent.click(screen.getByText("Checkout"));
  expect(screen.getByText("1 selected")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Checkout"));
  expect(screen.queryByText("1 selected")).not.toBeInTheDocument();
});

test("the keep-in-step refresh never reopens the browser", async () => {
  // The report page is a temp file the browser has open; the app keeps it
  // current by rewriting it in the background. That rewrite must go through
  // refresh_queue_html (write only) - it used to share view_queue_html with
  // the button, whose open_path opened ANOTHER tab every time the app window
  // regained focus.
  const sent: number[][] = [];
  let refreshes = 0;
  mockCases(
    (queue) => sent.push(queue.map((c) => c.update_id!)),
    () => {
      refreshes += 1;
    },
  );
  renderView();

  await screen.findByText("Login - valid");
  fireEvent.click(screen.getByRole("button", { name: "View All Test Cases in Browser" }));
  await waitFor(() => expect(sent).toHaveLength(1));

  // Coming back to the app re-reads notes and re-renders - the exact
  // trigger that used to open a fresh tab per alt-tab.
  fireEvent.click(screen.getByText("Login - valid"));
  window.dispatchEvent(new Event("focus"));

  // The 800ms debounce fires the background rewrite...
  await waitFor(() => expect(refreshes).toBeGreaterThan(0), { timeout: 3000 });
  // ...and the tab-opening command was never called again.
  expect(sent).toHaveLength(1);
});

test("Group by title folds cases under shared prefixes and persists collapse", async () => {
  mockCases();
  renderView();
  await screen.findByText("Login - valid");

  fireEvent.click(screen.getByRole("checkbox"));
  const header = await screen.findByRole("button", { name: "Login (2)" });

  // The header CHECKBOX selects the group.
  fireEvent.click(screen.getByRole("checkbox", { name: "Select all in Login" }));
  expect(screen.getByText("2 selected")).toBeInTheDocument();

  // The TITLE collapses - the name is the fold control now, same as the
  // chevron; state lands in localStorage for the next session.
  fireEvent.click(header);
  expect(screen.queryByText("Login - valid")).not.toBeInTheDocument();
  expect(screen.getByText("Checkout")).toBeInTheDocument();
  expect(JSON.parse(localStorage.getItem("tcm-v2-view-collapsed-groups")!)).toEqual(["Login"]);
});

/** Several details can be open at once, a collapsed group keeps the case
 * being READ on screen, and the sticky Collapse all clears the lot -
 * collapsing is tidying, and tidying must not snatch away the thing being
 * studied. */
test("open details survive a group collapse until Collapse all", async () => {
  mockCases();
  renderView();
  await screen.findByText("Login - valid");
  fireEvent.click(screen.getByRole("checkbox")); // Group by title

  // Grouping alone already gives the sticky something to fold: the open
  // groups count, before any detail is expanded (Login + Ungrouped).
  expect(screen.getByRole("button", { name: /Collapse all \(2\)/ })).toBeInTheDocument();

  // Two details open AT ONCE - comparing cases is the point of plural.
  fireEvent.click(screen.getByLabelText("Expand #201"));
  fireEvent.click(screen.getByLabelText("Expand #202"));
  expect(screen.getAllByText("Open login page")).toHaveLength(2);
  expect(screen.getByRole("button", { name: /Collapse all \(4\)/ })).toBeInTheDocument();

  // Collapse the Login group: the closed sibling rows vanish, the two
  // open details stay exactly where the reader left them.
  fireEvent.click(screen.getByLabelText("Collapse group Login"));
  expect(screen.getAllByText("Open login page")).toHaveLength(2);
  expect(screen.getByText("Login - valid")).toBeInTheDocument();

  // Collapse all folds EVERYTHING left - the open details and the still-
  // open Ungrouped group - and only then retires.
  fireEvent.click(screen.getByRole("button", { name: /Collapse all \(3\)/ }));
  expect(screen.queryByText("Open login page")).not.toBeInTheDocument();
  expect(screen.queryByText("Checkout")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Collapse all/ })).not.toBeInTheDocument();
});

/** The header checkbox is the one selection indicator: a dash (mixed) for a
 * partial selection, a tick for the whole group - expanded or collapsed.
 * There used to be a pulsing dot beside collapsed headings saying the same
 * thing; it said nothing the checkbox does not. */
test("the header checkbox carries the group's selection state through a collapse", async () => {
  mockCases();
  renderView();
  await screen.findByText("Login - valid");
  fireEvent.click(screen.getByRole("checkbox")); // Group by title

  // ONE case selected, not the whole group: the checkbox reads mixed.
  fireEvent.click(screen.getByText("Login - valid"));
  const loginBox = () => screen.getByRole("checkbox", { name: "Select all in Login" });
  expect(loginBox()).toHaveAttribute("aria-checked", "mixed");

  // Folding the group hides the highlighted rows - the header checkbox
  // stays on screen and keeps saying something is selected in there.
  fireEvent.click(screen.getByLabelText("Collapse group Login"));
  expect(loginBox()).toHaveAttribute("aria-checked", "mixed");
  // And no separate dot marker rides along any more.
  expect(screen.queryByRole("status")).not.toBeInTheDocument();

  // Completing the selection turns the dash into a tick, still collapsed.
  fireEvent.click(loginBox());
  expect(loginBox()).toHaveAttribute("aria-checked", "true");

  // Clearing it empties the box.
  fireEvent.click(loginBox());
  expect(loginBox()).toHaveAttribute("aria-checked", "false");
});



import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import WorkItemDrawer from "./WorkItemDrawer";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderDrawer() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <WorkItemDrawer
        org="acme"
        project="Web"
        itemId={2003}
        states={["To Do", "In Progress", "Done"]}
        onClose={vi.fn()}
        onSaved={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

const detail = (title: string) => ({
  id: 2003,
  title,
  work_item_type: "Bug",
  state: "To Do",
  assigned_to: "Avin Alwis",
  assigned_to_unique: "avin@acme.com",
  activity: "",
  tags: "",
  area_path: "Web",
  iteration_path: "Web\\Sprint 9",
  remaining_work: null,
  completed_work: null,
  original_estimate: null,
  start_date: "",
  target_date: "",
  description_text: "It broke",
  description_html: "<div>It broke</div>",
  description_field: "Microsoft.VSTS.TCM.ReproSteps",
  extra_pages: [
    {
      name: "RCA",
      fields: [
        {
          label: "Root Cause",
          reference_name: "Custom.RootCause",
          section: 0,
          kind: "html",
          allowed: [],
          value: "<div>TBD</div>",
        },
      ],
    },
  ],
  extra_pages_error: null,
  inline_images: [],
});

/// Saving invalidates the detail query, and fresh data used to run the
/// same reset that greets a DIFFERENT item - yanking whoever saved from
/// the RCA tab back to Description. The refetch must re-seed the draft
/// (the saved item is the new dirty-baseline) while the tabs stay put.
test("saving from the RCA tab stays on the RCA tab", async () => {
  let title = "Session timeout not enforced";
  mockIPC((cmd, args) => {
    switch (cmd) {
      case "work_item_detail":
        return detail(title);
      case "list_team_members":
        return [{ display_name: "Avin Alwis", unique_name: "avin@acme.com" }];
      case "activity_values":
        return [];
      case "work_item_comments":
        return [];
      case "update_work_item": {
        // The refetch after this save returns the edited title.
        const patches = (args as { patches: { reference_name: string; value: string }[] })
          .patches;
        title = patches.find((p) => p.reference_name === "System.Title")?.value ?? title;
        return null;
      }
    }
  });
  renderDrawer();

  // Open the RCA tab, then make the save button live with a title edit.
  fireEvent.click(await screen.findByRole("button", { name: "RCA" }));
  expect(screen.getByText("Root Cause")).toBeInTheDocument();
  fireEvent.change(screen.getByDisplayValue("Session timeout not enforced"), {
    target: { value: "Session timeout not enforced (repro'd)" },
  });

  fireEvent.click(screen.getByRole("button", { name: /save changes/i }));

  // The refetched detail landed (new title seeded the draft)...
  await waitFor(() =>
    expect(
      screen.getByDisplayValue("Session timeout not enforced (repro'd)"),
    ).toBeInTheDocument(),
  );
  // ...and the drawer is still on RCA, not bounced to Description.
  expect(screen.getByText("Root Cause")).toBeInTheDocument();
});

/// The detail a drawer loads is remembered on disk, so reopening the same
/// item paints instantly instead of a skeleton - the same seed-then-
/// revalidate treatment the suite tree and PR chips already get.
test("a loaded work item is cached for the next open", async () => {
  mockIPC((cmd) => {
    switch (cmd) {
      case "work_item_detail":
        return detail("Session timeout not enforced");
      case "list_team_members":
        return [];
      case "activity_values":
        return [];
      case "work_item_comments":
        return [];
    }
  });
  renderDrawer();
  await screen.findByDisplayValue("Session timeout not enforced");

  const raw = localStorage.getItem("tcm-v2-cache:wi-detail:acme/Web/2003");
  expect(raw).toBeTruthy();
  expect(JSON.parse(raw as string).data.title).toBe("Session timeout not enforced");
});

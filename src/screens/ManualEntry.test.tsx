import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import ManualEntry from "./ManualEntry";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const pbi = { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" };

function renderScreen(pbiArg: typeof pbi | null = pbi) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ManualEntry org="acme" project="Web" pbi={pbiArg} />
    </QueryClientProvider>,
  );
}

function baseMocks(handler: (cmd: string, args: unknown) => unknown = () => undefined) {
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return ["smoke", "regression"];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [{ id: 201, title: "Existing case", tags: "", automation_status: "Planned" }];
    return handler(cmd, args);
  });
}

function addCase(title: string) {
  fireEvent.change(screen.getByPlaceholderText("Test case title"), {
    target: { value: title },
  });
  // Fill step 1 via the shared StepsEditor grid, add + fill step 2.
  fireEvent.change(screen.getByLabelText("Step 1 action"), {
    target: { value: "Open page" },
  });
  fireEvent.change(screen.getByLabelText("Step 1 expected"), {
    target: { value: "Page shown" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add Step" }));
  fireEvent.change(screen.getByLabelText("Step 2 action"), {
    target: { value: "Submit form" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add to queue" }));
}

test("prompts for scope when no PBI is chosen", () => {
  baseMocks();
  renderScreen(null);
  expect(screen.getByText(/Pick an organization, project and PBI/)).toBeInTheDocument();
});

test("manual add, review gate, submit reports results", async () => {
  baseMocks((cmd, args) => {
    if (cmd === "submit_queue") {
      const a = args as { queue: Array<{ title: string }> };
      return a.queue.map((tc, index) => ({
        index,
        title: tc.title,
        action: "created",
        id: 900 + index,
        error: null,
      }));
    }
  });
  renderScreen();
  addCase("Login works");
  expect(await screen.findByText("Login works")).toBeInTheDocument();

  // One click into the review, and the check-the-PBI warning is already
  // up: the middle button used to say it would create and then did not.
  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  // The warning names the real cost of getting the PBI wrong. It used to
  // say created cases "cannot be deleted", which stopped being true the day
  // the recycle-bin delete shipped - and a test pinning a claim keeps it
  // alive long after the code stops backing it up.
  expect(screen.getByText(/needs delete permission/)).toBeInTheDocument();
  fireEvent.click(await screen.findByRole("button", { name: /Yes — create 1/ }));
  // The results panel leads with the headline, then names the case: a NEW
  // badge, the id, and the title as separate parts rather than one
  // run-together line.
  expect(await screen.findByText("1 test case uploaded - 1 created")).toBeInTheDocument();
  expect(screen.getByText("NEW")).toBeInTheDocument();
  expect(screen.getByText("#900")).toBeInTheDocument();

  // Clear results returns the screen to normal and removes itself.
  fireEvent.click(screen.getByRole("button", { name: "Clear results" }));
  expect(screen.queryByText("#900")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Clear results" })).not.toBeInTheDocument();
});

/// The per-row hint stays, and the fresh check now backs it: a title that
/// already exists on the PBI holds the write until someone has looked at
/// it. The hint alone was scrollable-past, which is how 43 duplicates once
/// went up.
test("a duplicate title warns in review and holds the write until accepted", async () => {
  baseMocks();
  renderScreen();
  addCase("Existing case");
  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  expect(await screen.findByText(/create a duplicate/)).toBeInTheDocument();

  const go = await screen.findByRole("button", { name: /Yes — create 1/ });
  await waitFor(() => expect(go).toBeDisabled());

  fireEvent.click(screen.getByRole("button", { name: "Create duplicates anyway" }));
  await waitFor(() => expect(go).toBeEnabled());
});

test("draft queue persists across remounts (shared with Import File)", async () => {
  baseMocks();
  const first = renderScreen();
  addCase("Persistent case");
  await screen.findByText("Persistent case");
  first.unmount();

  renderScreen();
  expect(await screen.findByText("Persistent case")).toBeInTheDocument();
});

test("the project's default tags ride on every case and cannot be removed here", async () => {
  localStorage.setItem("tcm-v2-default-tags:acme/Web", "smoke");
  baseMocks();
  renderScreen();

  // Present before any typing, and FIXED: no x, so a case cannot quietly
  // ship without the tag the project promised. Changing it is the
  // dialog's job, which is the one place that changes it for every case.
  expect(screen.getAllByText("smoke").length).toBeGreaterThan(0);
  expect(screen.queryByLabelText("Remove smoke")).not.toBeInTheDocument();

  addCase("Case carrying defaults");
  expect(await screen.findByText("Case carrying defaults")).toBeInTheDocument();

  // The reset goes back to the defaults, not to empty - the next case
  // wants them too, and still cannot drop them.
  expect(screen.queryByLabelText("Remove smoke")).not.toBeInTheDocument();
});

test("Default tags opens a dialog and saves the set for this project", async () => {
  baseMocks();
  renderScreen();

  fireEvent.click(screen.getByRole("button", { name: "Default tags" }));
  const field = await screen.findByLabelText("Default tags");
  fireEvent.change(field, { target: { value: "regression" } });
  fireEvent.keyDown(field, { key: "Enter" });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await waitFor(() =>
    expect(localStorage.getItem("tcm-v2-default-tags:acme/Web")).toBe("regression"),
  );
  // And it takes effect where it is used, without a reload.
  expect(screen.queryByLabelText("Remove regression")).not.toBeInTheDocument();
  expect(screen.getAllByText("regression").length).toBeGreaterThan(0);
});

test("tags added for one case clear when it is queued; the defaults do not", async () => {
  localStorage.setItem("tcm-v2-default-tags:acme/Web", "smoke");
  baseMocks();
  renderScreen();

  // An extra rides on top of the default and IS removable - the lock is
  // on the project's set, not on the field.
  const field = screen.getByLabelText("Tags");
  fireEvent.change(field, { target: { value: "one-off" } });
  fireEvent.keyDown(field, { key: "Enter" });
  expect(screen.getByLabelText("Remove one-off")).toBeInTheDocument();

  addCase("Case with an extra");
  expect(await screen.findByText("Case with an extra")).toBeInTheDocument();

  expect(screen.queryByLabelText("Remove one-off")).not.toBeInTheDocument();
  expect(screen.getAllByText("smoke").length).toBeGreaterThan(0);
});

test("cancelling the dialog changes nothing", async () => {
  localStorage.setItem("tcm-v2-default-tags:acme/Web", "smoke");
  baseMocks();
  renderScreen();

  fireEvent.click(screen.getByRole("button", { name: "Default tags" }));
  const field = await screen.findByLabelText("Default tags");
  fireEvent.change(field, { target: { value: "regression" } });
  fireEvent.keyDown(field, { key: "Enter" });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

  expect(localStorage.getItem("tcm-v2-default-tags:acme/Web")).toBe("smoke");
});

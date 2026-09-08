import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ExistingCases from "./ExistingCases";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderCases() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ExistingCases org="acme" project="Web" pbiId={42} />
    </QueryClientProvider>,
  );
}

const fullCase = {
  id: 201,
  title: "Valid login",
  tags: "smoke",
  automation_status: "Planned",
  steps: [
    { action: "Open page", expected: "Shown" },
    { action: "Submit", expected: "" },
  ],
  step_ids: ["2", "3"],
  module_value: "",
  preconditions: "",
};

const secondCase = { ...fullCase, id: 202, title: "Invalid login" };

test("expands a case via the chevron and saves edits via update_test_case", async () => {
  let updated: { tc?: { title: string; update_id: number | null } } = {};
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "update_test_case") {
      updated = args as typeof updated;
      return null;
    }
  });
  renderCases();

  await screen.findByText("Valid login");
  expect(screen.getByText("Valid login").closest("li")!.className).toContain("cv-row");
  fireEvent.click(screen.getByLabelText("Expand #201"));
  const titleInput = await screen.findByLabelText("Case title");
  // The editor's Combobox dropdown paints past the row, so an open row
  // must drop content-visibility's paint containment.
  expect(titleInput.closest("li")!.className).not.toContain("cv-row");
  fireEvent.change(titleInput, { target: { value: "Valid login v2" } });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

  await screen.findByText("Valid login"); // list still rendered
  expect(updated.tc?.title).toBe("Valid login v2");
  expect(updated.tc?.update_id).toBe(201);
});

/// Enter in the title = the Save button, under the same conditions: it
/// does nothing while the case is untouched, and saves once it is dirty.
test("Enter in the title saves a dirty case and ignores a clean one", async () => {
  let saves = 0;
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "update_test_case") {
      saves++;
      return null;
    }
  });
  renderCases();
  await screen.findByText("Valid login");
  fireEvent.click(screen.getByLabelText("Expand #201"));
  const title = await screen.findByLabelText("Case title");

  // Untouched: Enter must not fire a no-op PATCH.
  fireEvent.keyDown(title, { key: "Enter" });
  expect(saves).toBe(0);

  fireEvent.change(title, { target: { value: "Valid login v2" } });
  fireEvent.keyDown(title, { key: "Enter" });
  await vi.waitFor(() => expect(saves).toBe(1));
});

/** Discard is the way out of a half-made edit. It only exists once there
 *  is something to discard, and it puts the loaded values back. */
test("Discard changes appears only when edited, and reverts the edit", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "list_project_tags") return [];
  });
  renderCases();
  await screen.findByText("Valid login");
  fireEvent.click(screen.getByLabelText("Expand #201"));

  const title = await screen.findByLabelText("Case title");
  expect(screen.queryByRole("button", { name: /Discard changes/ })).not.toBeInTheDocument();

  fireEvent.change(title, { target: { value: "Valid login v2" } });
  fireEvent.click(screen.getByRole("button", { name: /Discard changes/ }));

  expect((await screen.findByLabelText("Case title")).getAttribute("value")).toBe("Valid login");
  // Back to clean, so the button retires with the change it undid.
  expect(screen.queryByRole("button", { name: /Discard changes/ })).not.toBeInTheDocument();
});

/** A step edit is a change too - the dirty check has to see into the steps
 *  array, not just compare the scalar fields. */
test("Discard also notices and reverts a step edit", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "list_project_tags") return [];
  });
  renderCases();
  await screen.findByText("Valid login");
  fireEvent.click(screen.getByLabelText("Expand #201"));

  const step = await screen.findByLabelText("Step 1 action");
  fireEvent.change(step, { target: { value: "Open the login page" } });
  fireEvent.click(await screen.findByRole("button", { name: /Discard changes/ }));

  expect((await screen.findByLabelText("Step 1 action")).getAttribute("value")).toBe("Open page");
});

/** After a save, the loaded values are stale - Azure DevOps holds what was
 *  just written. Discard must not offer to put the pre-save version back. */
test("a saved edit is the new baseline, not something to discard", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "list_project_tags") return [];
    if (cmd === "update_test_case") return null;
  });
  renderCases();
  await screen.findByText("Valid login");
  fireEvent.click(screen.getByLabelText("Expand #201"));

  fireEvent.change(await screen.findByLabelText("Case title"), {
    target: { value: "Valid login v2" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

  await waitFor(() =>
    expect(screen.queryByRole("button", { name: /Discard changes/ })).not.toBeInTheDocument(),
  );
  expect((await screen.findByLabelText("Case title")).getAttribute("value")).toBe("Valid login v2");
});

test("invalid edits disable save with a reason", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
  });
  renderCases();
  await screen.findByText("Valid login");
  fireEvent.click(screen.getByLabelText("Expand #201"));
  const titleInput = await screen.findByLabelText("Case title");
  fireEvent.change(titleInput, { target: { value: "   " } });
  expect(screen.getByRole("button", { name: "Save changes" })).toBeDisabled();
  expect(screen.getByText(/Title is required/)).toBeInTheDocument();
});

test("the header checkbox selects the group; title and chevron both collapse", async () => {
  localStorage.setItem("tcm-v2-group-cases", "on");
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full")
      return [
        { ...fullCase, id: 201, title: "Login - valid" },
        { ...fullCase, id: 202, title: "Login - locked out" },
        { ...fullCase, id: 203, title: "Standalone thing" },
      ];
  });
  renderCases();

  const header = await screen.findByRole("button", { name: "Login (2)" });
  const box = screen.getByRole("checkbox", { name: "Select all in Login" });
  fireEvent.click(box);
  expect(screen.getByText("2 selected")).toBeInTheDocument();

  // Clicking again clears the group's selection.
  fireEvent.click(box);
  expect(screen.queryByText("2 selected")).not.toBeInTheDocument();

  // The TITLE collapses the group's cards (header stays)...
  fireEvent.click(header);
  expect(screen.queryByText("Login - valid")).not.toBeInTheDocument();
  expect(screen.getByText("Standalone thing")).toBeInTheDocument();
  // ...and the chevron reopens it - both are fold controls now.
  fireEvent.click(screen.getByLabelText("Expand group Login"));
  expect(screen.getByText("Login - valid")).toBeInTheDocument();
});

test("card clicks drive multi-select and unlock the bulk toolbar", async () => {
  const updatedIds: number[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase, secondCase];
    if (cmd === "update_test_case") {
      const a = args as { tc: { update_id: number | null } };
      if (a.tc.update_id != null) updatedIds.push(a.tc.update_id);
      return null;
    }
  });
  renderCases();

  // Single click selects one; ctrl+click adds the second.
  fireEvent.click(await screen.findByText("Valid login"));
  expect(screen.getByText("1 selected")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Invalid login"), { ctrlKey: true });
  expect(screen.getByText("2 selected")).toBeInTheDocument();

  // Bulk edit both: pick a status, apply serially.
  fireEvent.click(screen.getByRole("button", { name: "Bulk edit" }));
  fireEvent.click(screen.getByLabelText(/Automation status/));
  fireEvent.click(screen.getByRole("option", { name: "Not Automated" }));
  fireEvent.click(screen.getByRole("button", { name: "Apply to 2" }));
  await waitFor(() => expect(updatedIds).toHaveLength(2));
  expect([...updatedIds].sort()).toEqual([201, 202]);
});

/** Backing out of a bulk dialog must not undo the work of choosing what to
 *  bulk-edit. Power Rename's close handler used to clear the selection, so
 *  Cancel left the user re-picking every case to try again. */
test("cancelling Power Rename keeps the selection", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase, secondCase];
    if (cmd === "list_project_tags") return [];
  });
  renderCases();

  fireEvent.click(await screen.findByText("Valid login"));
  fireEvent.click(screen.getByText("Invalid login"), { ctrlKey: true });
  expect(screen.getByText("2 selected")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Power Rename" }));
  fireEvent.click(await screen.findByRole("button", { name: /Cancel/ }));

  expect(screen.getByText("2 selected")).toBeInTheDocument();
  // And the dialog really did close, so this is not just a stale render.
  expect(screen.queryByRole("button", { name: /Cancel/ })).not.toBeInTheDocument();
});

test("search narrows the list by title, id or tag", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full")
      return [fullCase, secondCase, { ...fullCase, id: 203, title: "Checkout", tags: "regression" }];
  });
  renderCases();
  await screen.findByText("Valid login");

  const box = screen.getByLabelText("Search test cases");

  // Title match: only the checkout case stays.
  fireEvent.change(box, { target: { value: "checkout" } });
  expect(screen.queryByText("Valid login")).not.toBeInTheDocument();
  expect(screen.getByText("Checkout")).toBeInTheDocument();

  // Id match.
  fireEvent.change(box, { target: { value: "#202" } });
  expect(screen.getByText("Invalid login")).toBeInTheDocument();
  expect(screen.queryByText("Checkout")).not.toBeInTheDocument();

  // Tag match.
  fireEvent.change(box, { target: { value: "regression" } });
  expect(screen.getByText("Checkout")).toBeInTheDocument();

  // No match shows the empty hint; clearing restores everything.
  fireEvent.change(box, { target: { value: "zzz" } });
  expect(screen.getByText(/No test cases match/)).toBeInTheDocument();
  fireEvent.change(box, { target: { value: "" } });
  expect(screen.getByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("Checkout")).toBeInTheDocument();
});

/** Same merged sticky as View Test Cases / Run Tests, adjusted for this
 * screen's single-editor model: one press folds the open editor and every
 * unfolded group, then the button retires. */
test("sticky Collapse all folds the open editor and every open group", async () => {
  localStorage.setItem("tcm-v2-group-cases", "on");
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full")
      return [
        { ...fullCase, id: 201, title: "Login - valid" },
        { ...fullCase, id: 202, title: "Login - locked out" },
        { ...fullCase, id: 203, title: "Standalone thing" },
      ];
  });
  renderCases();
  await screen.findByRole("button", { name: "Login (2)" });

  // Grouping alone gives the sticky its fold targets (Login + Ungrouped).
  expect(screen.getByRole("button", { name: /Collapse all \(2\)/ })).toBeInTheDocument();

  // An open editor is one more thing to fold.
  fireEvent.click(screen.getByLabelText("Expand #201"));
  expect(await screen.findByLabelText("Case title")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /Collapse all \(3\)/ }));

  // Editor gone, groups folded, button retired.
  expect(screen.queryByLabelText("Case title")).not.toBeInTheDocument();
  expect(screen.queryByText("Login - valid")).not.toBeInTheDocument();
  expect(screen.queryByText("Standalone thing")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Collapse all/ })).not.toBeInTheDocument();
});

/** The title is a fold control now, and folding must not become a silent
 * discard: an open editor (possibly carrying unsaved edits) stays mounted
 * when its group collapses - only the closed sibling rows go. */
test("folding a group keeps the open editor and its unsaved edits", async () => {
  localStorage.setItem("tcm-v2-group-cases", "on");
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full")
      return [
        { ...fullCase, id: 201, title: "Login - valid" },
        { ...fullCase, id: 202, title: "Login - locked out" },
        { ...fullCase, id: 203, title: "Standalone thing" },
      ];
  });
  renderCases();

  const header = await screen.findByRole("button", { name: "Login (2)" });
  fireEvent.click(screen.getByLabelText("Expand #201"));
  const title = await screen.findByLabelText("Case title");
  fireEvent.change(title, { target: { value: "Login - valid EDITED" } });

  // Fold the group via the title: the sibling row vanishes, the case
  // being edited stays, edit intact.
  fireEvent.click(header);
  expect(screen.queryByText("Login - locked out")).not.toBeInTheDocument();
  expect((screen.getByLabelText("Case title") as HTMLInputElement).value).toBe(
    "Login - valid EDITED",
  );
});

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
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
  module_value: "",
  preconditions: "",
};

test("expands a case and saves edits via update_test_case", async () => {
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

  fireEvent.click(await screen.findByText("Valid login"));
  const titleInput = await screen.findByLabelText("Case title");
  fireEvent.change(titleInput, { target: { value: "Valid login v2" } });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

  await screen.findByText("Valid login"); // list still rendered
  expect(updated.tc?.title).toBe("Valid login v2");
  expect(updated.tc?.update_id).toBe(201);
});

test("invalid edits disable save with a reason", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
  });
  renderCases();
  fireEvent.click(await screen.findByText("Valid login"));
  const titleInput = await screen.findByLabelText("Case title");
  fireEvent.change(titleInput, { target: { value: "   " } });
  expect(screen.getByRole("button", { name: "Save changes" })).toBeDisabled();
  expect(screen.getByText(/Title is required/)).toBeInTheDocument();
});

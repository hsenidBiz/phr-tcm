import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import AiGuideWizard from "./AiGuideWizard";

// The directory picker is a plugin call; mock the module, not IPC.
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => "C:/repo") }));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

function renderWizard(onClose = vi.fn()) {
  localStorage.setItem(
    "tcm-v2-fields:acme/Web",
    JSON.stringify({ moduleRef: "Custom.Module", preconditionsRef: null }),
  );
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <AiGuideWizard org="acme" project="Web" area="Web\\Gamma" onClose={onClose} />
    </QueryClientProvider>,
  );
  return onClose;
}

test("step 1 shows discovered modules and tags as checked boxes; unticking prunes", async () => {
  mockIPC((cmd) => {
    if (cmd === "test_case_field_values") return ["Login", "Checkout"];
    if (cmd === "list_project_tags") return ["smoke"];
  });
  renderWizard();
  expect(await screen.findByLabelText("Login")).toBeChecked();
  expect(screen.getByLabelText("Checkout")).toBeChecked();
  expect(screen.getByLabelText("smoke")).toBeChecked();
  fireEvent.click(screen.getByLabelText("Checkout")); // prune it
  expect(screen.getByLabelText("Checkout")).not.toBeChecked();
});

test("discovery failure degrades with a note, not a blocked wizard", async () => {
  mockIPC((cmd) => {
    if (cmd === "test_case_field_values") throw "boom";
    if (cmd === "list_project_tags") return [];
  });
  renderWizard();
  expect(await screen.findByText(/could not be discovered/i)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();
});

test("full walk-through: doc paths + flavors reach writeAiGuide with the pruned payload", async () => {
  let writeArgs: Record<string, unknown> | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "test_case_field_values") return ["Login", "Checkout"];
    if (cmd === "list_project_tags") return [];
    if (cmd === "write_ai_guide") {
      writeArgs = args as Record<string, unknown>;
      return ["AI_TEST_CASES.md"];
    }
  });
  const onClose = renderWizard();

  fireEvent.click(await screen.findByLabelText("Checkout")); // prune
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> repo knowledge
  fireEvent.change(screen.getByLabelText("Documentation paths"), {
    target: { value: "docs/screens/**\nREADME.md" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> flavor
  expect(screen.getByLabelText("Generic markdown (AI_TEST_CASES.md)")).toBeChecked();
  fireEvent.click(screen.getByLabelText("Claude Code skill"));
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> output
  fireEvent.click(screen.getByRole("button", { name: "Save to folder…" }));

  await waitFor(() => expect(writeArgs).not.toBeNull());
  const opts = (writeArgs as unknown as { options: Record<string, unknown> }).options;
  expect(opts.modules).toEqual(["Login"]); // pruned
  expect(opts.docPaths ?? opts.doc_paths).toEqual(["docs/screens/**", "README.md"]);
  expect((opts.flavors as string[]).length).toBe(2);
  await screen.findByText(/AI_TEST_CASES\.md/); // success summary
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(onClose).toHaveBeenCalled();
});

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

test("step 1 offers modules as search-and-add; picking a suggestion makes a chip", async () => {
  mockIPC((cmd) => {
    if (cmd === "test_case_field_values") return ["Login", "Checkout", "Payroll India"];
    if (cmd === "list_repos") return [];
  });
  renderWizard();
  const input = await screen.findByLabelText("Modules");
  // Typing filters the 100+-value picklist instead of rendering it whole.
  fireEvent.change(input, { target: { value: "log" } });
  expect(screen.getByRole("button", { name: "Login" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Payroll India" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Login" }));
  expect(screen.getByRole("button", { name: "Remove Login" })).toBeInTheDocument(); // chip
  // Tags are deliberately NOT offered - large orgs have hundreds.
  expect(screen.queryByText("Tags")).not.toBeInTheDocument();
});

test("discovery failure degrades with a note, not a blocked wizard", async () => {
  mockIPC((cmd) => {
    if (cmd === "test_case_field_values") throw "boom";
    if (cmd === "list_repos") return [];
  });
  renderWizard();
  expect(await screen.findByText(/could not be discovered/i)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();
});

test("the docs step browses a repo's folders and picks one as a doc path", async () => {
  let writeArgs: Record<string, unknown> | null = null;
  const folderCalls: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "test_case_field_values") return [];
    if (cmd === "list_repos")
      return [
        { id: "r1", name: "web" },
        { id: "r2", name: "api" },
      ];
    if (cmd === "list_repo_folders") {
      const path = (args as { path: string }).path;
      folderCalls.push(path);
      if (path === "/") return ["/Prototype", "/src"];
      if (path === "/Prototype") return [];
      return [];
    }
    if (cmd === "write_ai_guide") {
      writeArgs = args as Record<string, unknown>;
      return ["AI_TEST_CASES.md"];
    }
  });
  renderWizard();
  fireEvent.click(await screen.findByRole("button", { name: "Next" })); // -> docs step

  // Pick a repo -> its root folders load.
  fireEvent.change(screen.getByLabelText("Repository"), { target: { value: "r1" } });
  fireEvent.click(await screen.findByRole("button", { name: "Prototype/" })); // descend
  await waitFor(() => expect(folderCalls).toContain("/Prototype"));
  fireEvent.click(screen.getByRole("button", { name: "Use this folder" }));
  expect(screen.getByText("Prototype/**")).toBeInTheDocument(); // picked list

  // Finish the wizard and confirm the picked folder reaches the payload.
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> flavors
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> output
  fireEvent.click(screen.getByRole("button", { name: "Save to folder…" }));
  await waitFor(() => expect(writeArgs).not.toBeNull());
  const opts = (writeArgs as unknown as { options: Record<string, unknown> }).options;
  expect(opts.doc_paths).toEqual(["Prototype/**"]);
});

test("full walk-through: manual doc paths + flavors reach writeAiGuide with the pruned payload", async () => {
  let writeArgs: Record<string, unknown> | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "test_case_field_values") return ["Login", "Checkout"];
    if (cmd === "list_repos") return [];
    if (cmd === "write_ai_guide") {
      writeArgs = args as Record<string, unknown>;
      return ["AI_TEST_CASES.md"];
    }
  });
  const onClose = renderWizard();

  const input = await screen.findByLabelText("Modules");
  fireEvent.focus(input);
  fireEvent.click(await screen.findByRole("button", { name: "Login" })); // add just Login
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
  expect(opts.modules).toEqual(["Login"]); // only the explicitly added module
  expect(opts.doc_paths).toEqual(["docs/screens/**", "README.md"]);
  expect((opts.flavors as string[]).length).toBe(2);
  await screen.findByText(/AI_TEST_CASES\.md/); // success summary
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(onClose).toHaveBeenCalled();
});

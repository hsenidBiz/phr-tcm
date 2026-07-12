import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import BugDialog from "./BugDialog";

afterEach(() => clearMocks());

const testCase = {
  id: 201,
  title: "Valid login",
  tags: "",
  automation_status: "Planned",
  steps: [{ action: "Open page", expected: "Shown" }],
  step_ids: ["2"],
  module_value: "",
  preconditions: "",
};

test("prefills repro from the case and files a linked bug", async () => {
  let filed: Record<string, unknown> = {};
  mockIPC((cmd, args) => {
    if (cmd === "file_bug") {
      filed = args as Record<string, unknown>;
      return { id: 555, url: "https://x/bug/555" };
    }
  });
  const onFiled = vi.fn();
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <BugDialog
        org="acme"
        project="Web"
        testCase={testCase}
        pbiId={42}
        screenshots={["AAAA"]}
        onClose={vi.fn()}
        onFiled={onFiled}
      />
    </QueryClientProvider>,
  );

  expect(screen.getByLabelText("Bug title")).toHaveValue("Bug: Valid login");
  expect((screen.getByLabelText("Repro steps") as HTMLTextAreaElement).value).toContain(
    "Test case #201",
  );

  fireEvent.click(screen.getByRole("button", { name: "File bug" }));
  await vi.waitFor(() => expect(onFiled).toHaveBeenCalledWith(555));
  expect(filed.testCaseId).toBe(201);
  expect(filed.pbiId).toBe(42);
  expect(filed.screenshotsB64).toEqual(["AAAA"]);
});

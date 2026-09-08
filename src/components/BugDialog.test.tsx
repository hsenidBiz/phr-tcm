import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import BugDialog from "./BugDialog";

afterEach(() => clearMocks());

const testCase = {
  steps_xml: "",
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

/// A capture sitting on the clipboard should not have to detour through
/// the evidence card: pasting into the dialog attaches it to the bug.
test("pasting an image into the dialog files it with the bug", async () => {
  let filed: Record<string, unknown> = {};
  mockIPC((cmd, args) => {
    if (cmd === "file_bug") {
      filed = args as Record<string, unknown>;
      return { id: 556, url: "https://x/bug/556" };
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <BugDialog
        org="acme"
        project="Web"
        testCase={testCase}
        pbiId={42}
        screenshots={[]}
        onClose={vi.fn()}
        onFiled={vi.fn()}
      />
    </QueryClientProvider>,
  );

  const file = new File([new Uint8Array([105, 109, 103])], "clip.png", { type: "image/png" });
  fireEvent.paste(screen.getByLabelText("Bug title"), {
    clipboardData: { items: [{ type: "image/png", getAsFile: () => file }] },
  });

  // Thumbnail with a remove control appears...
  expect(await screen.findByAltText("Pasted screenshot 1")).toBeInTheDocument();
  expect(screen.getByLabelText("Remove pasted screenshot 1")).toBeInTheDocument();

  // ...and the filed bug carries the pasted bytes ("img" -> "aW1n").
  fireEvent.click(screen.getByRole("button", { name: "File bug" }));
  await vi.waitFor(() => expect(filed.screenshotsB64).toEqual(["aW1n"]));
});

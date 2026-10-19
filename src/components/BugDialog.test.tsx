import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import BugDialog from "./BugDialog";
import { renderCaseMarkdown } from "../lib/markdown";

afterEach(() => clearMocks());

const testCase = {
  steps_xml: "",
  priority: 2,
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

test("a Shared Steps row goes into the repro as Shared steps #N, with its title when known", () => {
  mockIPC(() => null);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  qc.setQueryData(["shared-step", "acme", 812], "Sign in as an admin");
  render(
    <QueryClientProvider client={qc}>
      <BugDialog
        org="acme"
        project="Web"
        testCase={{
          ...testCase,
          steps: [
            { action: "Open page", expected: "Shown" },
            { action: "", expected: "", shared: 812 },
            { action: "", expected: "", shared: 900 },
          ],
          step_ids: ["2", "", ""],
        }}
        pbiId={42}
        screenshots={[]}
        onClose={vi.fn()}
        onFiled={vi.fn()}
      />
    </QueryClientProvider>,
  );
  const repro = (screen.getByLabelText("Repro steps") as HTMLTextAreaElement).value;
  expect(repro).toContain("1. Open page\n   Expected: Shown");
  expect(repro).toContain("2. Shared steps #812 - Sign in as an admin");
  expect(repro).toContain("3. Shared steps #900");
  expect(repro).not.toMatch(/^\d+\. $/m);
});

/// The repro goes to Azure DevOps as rich text (the prerequisites' HTML).
/// Each step is a list item with its other lines indented under it, so a
/// fenced query inside a step closes inside that step: the expected result
/// and the steps after it are not swallowed into the code block.
test("a step holding a fenced query keeps the next step out of its code block", () => {
  mockIPC(() => null);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <BugDialog
        org="acme"
        project="Web"
        testCase={{
          ...testCase,
          steps: [
            { action: "Run the query:\n\n```sql\nSELECT 1\n```", expected: "one row" },
            { action: "Open the report", expected: "It shows the row" },
          ],
          step_ids: ["2", "3"],
        }}
        pbiId={42}
        screenshots={[]}
        onClose={vi.fn()}
        onFiled={vi.fn()}
      />
    </QueryClientProvider>,
  );
  const repro = (screen.getByLabelText("Repro steps") as HTMLTextAreaElement).value;
  const doc = new DOMParser().parseFromString(renderCaseMarkdown(repro, "prereq"), "text/html");
  const pres = doc.querySelectorAll("pre");
  expect(pres).toHaveLength(1);
  expect(pres[0].textContent?.trim()).toBe("SELECT 1");
  const items = doc.querySelectorAll("ol > li");
  expect(items).toHaveLength(2);
  expect(items[0].textContent).toContain("Expected: one row");
  expect(items[0].querySelector("pre")).not.toBeNull();
  expect(items[1].textContent).toContain("Open the report");
  expect(items[1].textContent).toContain("Expected: It shows the row");
  expect(items[1].querySelector("pre")).toBeNull();
});

test("an expected result that starts with a fence is its own block under Expected:", () => {
  mockIPC(() => null);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <BugDialog
        org="acme"
        project="Web"
        testCase={{
          ...testCase,
          steps: [
            { action: "Run the check", expected: "```\nNo rows\n```" },
            { action: "Close the page", expected: "" },
          ],
          step_ids: ["2", "3"],
        }}
        pbiId={42}
        screenshots={[]}
        onClose={vi.fn()}
        onFiled={vi.fn()}
      />
    </QueryClientProvider>,
  );
  const repro = (screen.getByLabelText("Repro steps") as HTMLTextAreaElement).value;
  const doc = new DOMParser().parseFromString(renderCaseMarkdown(repro, "prereq"), "text/html");
  expect([...doc.querySelectorAll("pre")].map((p) => p.textContent?.trim())).toEqual(["No rows"]);
  const items = doc.querySelectorAll("ol > li");
  expect(items).toHaveLength(2);
  expect(items[1].textContent?.trim()).toBe("Close the page");
});

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import BugDialog from "./BugDialog";
import { renderCaseMarkdown } from "../lib/markdown";

// The toasts are asserted on (a broken paste must never claim success).
const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  info: vi.fn(),
  warning: vi.fn(),
  dismiss: vi.fn(),
}));
vi.mock("../lib/toast", () => ({ toast: toastMock }));

// jsdom cannot decode an image, so the decoder validImageB64 uses accepts
// whatever the structural checks let through.
vi.mock("../lib/blob", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/blob")>();
  return {
    ...actual,
    validImageB64: (b64: string, mime?: string) => actual.validImageB64(b64, mime, async () => true),
  };
});

/** A real 1x1 PNG (69 bytes), base64. */
const PNG_B64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
const pngBytes = () => Uint8Array.from(atob(PNG_B64), (c) => c.charCodeAt(0));

afterEach(() => {
  clearMocks();
  for (const m of Object.values(toastMock)) m.mockClear();
});

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

  const file = new File([pngBytes() as BlobPart], "clip.png", { type: "image/png" });
  fireEvent.paste(screen.getByLabelText("Bug title"), {
    clipboardData: { items: [{ type: "image/png", getAsFile: () => file }] },
  });

  // Thumbnail with a remove control appears...
  expect(await screen.findByAltText("Pasted screenshot 1")).toBeInTheDocument();
  expect(screen.getByLabelText("Remove pasted screenshot 1")).toBeInTheDocument();

  // ...and the filed bug carries the pasted picture, whole.
  fireEvent.click(screen.getByRole("button", { name: "File bug" }));
  await vi.waitFor(() => expect(filed.screenshotsB64).toEqual([PNG_B64]));
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

// ---- A screenshot that has not reached the clipboard yet ----
//
// The same quick paste from a virtual machine, inside File a bug: the file
// is empty or cut off. It used to be added as it came (a broken thumbnail,
// "Screenshot added to the bug") and filed with the bug.

function renderBugDialog() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
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
}

function pasteImage(bytes: Uint8Array, type = "image/png") {
  const file = new File([bytes as BlobPart], "clip", { type });
  fireEvent.paste(screen.getByLabelText("Bug title"), {
    clipboardData: { items: [{ type, getAsFile: () => file }] },
  });
}

const NOT_YET = "The screenshot had not reached the clipboard yet - paste again.";

test("an empty pasted image is not added to the bug and the person is told", async () => {
  mockIPC(() => null);
  renderBugDialog();

  pasteImage(new Uint8Array(0));

  await vi.waitFor(() => expect(toastMock.info).toHaveBeenCalledWith(NOT_YET));
  expect(screen.queryByAltText("Pasted screenshot 1")).not.toBeInTheDocument();
  expect(toastMock.success).not.toHaveBeenCalled();
  expect(screen.queryByText(/screenshot\(s\) attached/)).not.toBeInTheDocument();
});

test("a cut-off pasted image is not added either", async () => {
  mockIPC(() => null);
  renderBugDialog();

  pasteImage(pngBytes().slice(0, 40));

  await vi.waitFor(() => expect(toastMock.info).toHaveBeenCalledWith(NOT_YET), { timeout: 5000 });
  expect(screen.queryByAltText("Pasted screenshot 1")).not.toBeInTheDocument();
  expect(toastMock.success).not.toHaveBeenCalled();
});

test("a pasted JPEG is added under its own type", async () => {
  mockIPC(() => null);
  renderBugDialog();
  const jpeg = new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 0, 16, 0x4a, 0x46, 0x49, 0x46, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0xff, 0xd9]);

  pasteImage(jpeg, "image/jpeg");

  const thumb = await screen.findByAltText("Pasted screenshot 1");
  expect(thumb).toHaveAttribute("src", `data:image/jpeg;base64,${btoa(String.fromCharCode(...jpeg))}`);
  expect(toastMock.success).toHaveBeenCalledWith("Screenshot added to the bug");
});

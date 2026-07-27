/** The step-log viewer: it has to show the whole log, and hand it over. */
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { LogDialog, logLineTone } from "./PipelineDialog";

const copyText = vi.fn(async (_text: string) => {});
vi.mock("../lib/clipboard", () => ({ copyText: (t: string) => copyText(t) }));

afterEach(() => {
  clearMocks();
  copyText.mockClear();
});

const LOG = "2026-07-24T09:00:00Z Starting: Build\n##[error]it broke\nPassed! - Failed: 0";

function renderLog(live = false) {
  mockIPC((cmd) => {
    if (cmd === "build_log") return LOG;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <LogDialog
        org="acme"
        project="Web"
        buildId={901}
        title="Run Unit Test"
        logId={42}
        live={live}
        onClose={() => {}}
      />
    </QueryClientProvider>,
  );
}

test("the log renders with its lines numbered and toned", async () => {
  renderLog();
  // The ##[..] marker stays on the line, exactly as Azure DevOps shows it -
  // only the leading timestamp is split off to be dimmed.
  expect(await screen.findByText("##[error]it broke")).toBeInTheDocument();
  // The leading timestamp is split off and dimmed, not left inline.
  expect(screen.getByText("2026-07-24T09:00:00Z")).toBeInTheDocument();
  expect(screen.getByText("Starting: Build")).toBeInTheDocument();
});

test("the copy button hands over the RAW log, not the rendered lines", async () => {
  renderLog();
  // The button is disabled until there is something to copy, so wait for
  // the log before clicking it.
  await screen.findByText("##[error]it broke");
  fireEvent.click(screen.getByRole("button", { name: "Copy log" }));

  await waitFor(() => expect(copyText).toHaveBeenCalledWith(LOG));
  // Confirmation is on the button itself - no toast to chase.
  expect(await screen.findByText("Copied")).toBeInTheDocument();
});

test("copying is disabled while there is nothing to copy", async () => {
  mockIPC((cmd) => {
    if (cmd === "build_log") return "";
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <LogDialog
        org="acme"
        project="Web"
        buildId={901}
        title="Empty step"
        logId={7}
        live={false}
        onClose={() => {}}
      />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Copy log" })).toBeDisabled(),
  );
});

/** The viewer should follow the window rather than stopping at a fixed
 * width - build log lines are long. */
test("the dialog is sized from the viewport, with no width cap", async () => {
  renderLog();
  await screen.findByText("##[error]it broke");
  // The modal renders through a portal, so search the document.
  const panel = document.querySelector('[class*="w-[94vw]"]');
  expect(panel).toBeTruthy();
  expect(panel!.className).not.toMatch(/max-w-/);
});

test("log lines are coloured by their markers, success before failure", () => {
  expect(logLineTone("##[error]boom")).toBe("text-danger");
  expect(logLineTone("##[warning]hmm")).toBe("text-warning");
  // A summary line naming both must read as the pass it is.
  expect(logLineTone("Passed! - Failed: 0")).toBe("text-success");
  expect(logLineTone("plain output")).toBe("text-muted");
});

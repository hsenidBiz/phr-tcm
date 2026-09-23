import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ErrorBoundary, { installGlobalErrorLog } from "./ErrorBoundary";

afterEach(() => clearMocks());

function Bomb(): never {
  throw new Error("reviewRows[i] is undefined");
}

/// A render that throws used to take the whole window with it - white
/// screen, nothing logged. Now the boundary draws the failure, offers a
/// reload, and writes the message and stacks into the app log through
/// the UI-breadcrumb command, so a bug report carries what happened.
test("a render crash shows a fallback and is written to the app log", () => {
  const logged: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "log_ui") logged.push((args as { message: string }).message);
  });
  // React logs the caught error to console.error; keep the test output clean.
  const quiet = vi.spyOn(console, "error").mockImplementation(() => {});
  try {
    render(
      <ErrorBoundary>
        <Bomb />
      </ErrorBoundary>,
    );
  } finally {
    quiet.mockRestore();
  }

  expect(screen.getByRole("alert")).toHaveTextContent("Something went wrong in the app");
  expect(screen.getByText("reviewRows[i] is undefined")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Reload the app" })).toBeInTheDocument();
  expect(logged).toHaveLength(1);
  expect(logged[0]).toContain("render crash: reviewRows[i] is undefined");
  expect(logged[0]).toContain("component stack:");
});

/// Recording an unhandled rejection must not raise one of its own. The log
/// call is async, and when it failed (no bridge in a plain browser, or the
/// IPC call erroring) its rejected promise went unhandled - which fired the
/// same hook, which logged again, which failed again: an endless loop that
/// kept a CPU core busy for as long as the window was open.
test("a failing log call is swallowed, not turned into another unhandled rejection", async () => {
  let calls = 0;
  mockIPC((cmd) => {
    if (cmd === "log_ui") {
      calls++;
      throw new Error("no bridge");
    }
  });
  const unhandled: unknown[] = [];
  const onUnhandled = (reason: unknown) => unhandled.push(reason);
  process.on("unhandledRejection", onUnhandled);
  try {
    installGlobalErrorLog();
    window.dispatchEvent(Object.assign(new Event("unhandledrejection"), { reason: new Error("boom") }));
    // Let the failed log call settle, and anything it would have raised.
    await new Promise((r) => setTimeout(r, 50));
  } finally {
    process.off("unhandledRejection", onUnhandled);
  }
  expect(calls).toBe(1);
  expect(unhandled).toEqual([]);
});

test("without an error the children render as they are", () => {
  render(
    <ErrorBoundary>
      <p>all good</p>
    </ErrorBoundary>,
  );
  expect(screen.getByText("all good")).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

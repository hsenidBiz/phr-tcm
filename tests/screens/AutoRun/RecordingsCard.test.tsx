// The Recordings card: kept recordings listed with Play, Save and Remove.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const dialog = vi.hoisted(() => ({ save: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
const toastMock = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn(), info: vi.fn(), warning: vi.fn(), dismiss: vi.fn() }));
vi.mock("../../../src/lib/toast", () => ({ toast: toastMock }));

import RecordingsCard, { REMOVE_CONFIRM } from "../../../src/screens/AutoRun/RecordingsCard";
import { VIDEO_GONE } from "../../../src/screens/AutoRun/CaseVideo";

const ENTRY = {
  id: "recording-1764238500000-000001",
  title: "Bug 4521: leave balance fix",
  created: "2026-11-27T10:15:00Z",
  duration_ms: 83000,
  file: "recording-1764238500000-000001.mp4",
};

beforeEach(() => {
  URL.createObjectURL = vi.fn(() => "blob:the-recording");
  URL.revokeObjectURL = vi.fn();
});

afterEach(() => {
  cleanup();
  clearMocks();
  dialog.save.mockReset();
  for (const m of Object.values(toastMock)) m.mockClear();
});

function show() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <RecordingsCard />
    </QueryClientProvider>,
  );
}

test("nothing shows while there are no recordings", async () => {
  const asked: string[] = [];
  mockIPC((cmd) => {
    asked.push(cmd);
    return [];
  });
  show();
  await waitFor(() => expect(asked).toContain("auto_run_recordings"));
  expect(screen.queryByText("Recordings")).toBeNull();
});

test("each recording shows its title, time and length", async () => {
  mockIPC((cmd) => (cmd === "auto_run_recordings" ? [ENTRY] : null));
  show();
  expect(await screen.findByRole("heading", { name: "Recordings" })).toBeInTheDocument();
  expect(screen.getByText(ENTRY.title)).toBeInTheDocument();
  expect(screen.getByText("1:23")).toBeInTheDocument();
});

test("Play shows the recording in the app", async () => {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "auto_run_recordings") return [ENTRY];
    if (cmd === "auto_run_video") return btoa("abc");
    return null;
  });
  show();
  fireEvent.click(await screen.findByRole("button", { name: `Play ${ENTRY.title}` }));
  expect(await screen.findByLabelText(`Recording: ${ENTRY.title}`)).toBeInTheDocument();
  expect(calls.find((c) => c.cmd === "auto_run_video")?.args).toEqual({ name: ENTRY.file });
});

test("Save asks where and copies the file there", async () => {
  const calls: { cmd: string; args: unknown }[] = [];
  dialog.save.mockResolvedValue("C:/out/x.mp4");
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return cmd === "auto_run_recordings" ? [ENTRY] : null;
  });
  show();
  fireEvent.click(await screen.findByRole("button", { name: `Save ${ENTRY.title}` }));
  await waitFor(() => expect(calls.some((c) => c.cmd === "auto_run_save_video")).toBe(true));
  expect(dialog.save.mock.calls[0][0].defaultPath).toBe("Bug 4521 leave balance fix.mp4");
  expect(calls.find((c) => c.cmd === "auto_run_save_video")?.args).toEqual({ name: ENTRY.file, destPath: "C:/out/x.mp4" });
});

test("Remove asks first and then removes it", async () => {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return cmd === "auto_run_recordings" ? [ENTRY] : null;
  });
  show();
  fireEvent.click(await screen.findByRole("button", { name: `Remove ${ENTRY.title}` }));
  expect(screen.getByText(REMOVE_CONFIRM)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Keep" }));
  expect(screen.queryByText(REMOVE_CONFIRM)).toBeNull();
  expect(calls.some((c) => c.cmd === "auto_run_remove_recording")).toBe(false);

  fireEvent.click(screen.getByRole("button", { name: `Remove ${ENTRY.title}` }));
  fireEvent.click(screen.getByRole("button", { name: "Remove recording" }));
  await waitFor(() => expect(calls.some((c) => c.cmd === "auto_run_remove_recording")).toBe(true));
  expect(calls.find((c) => c.cmd === "auto_run_remove_recording")?.args).toEqual({ id: ENTRY.id });
  await waitFor(() => expect(calls.filter((c) => c.cmd === "auto_run_recordings").length).toBeGreaterThan(1));
});

test("a title with slashes or a device name gives a safe default file name", async () => {
  dialog.save.mockResolvedValue(null);
  for (const [title, want] of [["Fix C:\\Payroll\\export", "Fix CPayrollexport.mp4"], ["CON", "recording.mp4"]]) {
    mockIPC((cmd) => (cmd === "auto_run_recordings" ? [{ ...ENTRY, title }] : null));
    const { unmount } = show();
    fireEvent.click(await screen.findByRole("button", { name: `Save ${title}` }));
    await waitFor(() => expect(dialog.save).toHaveBeenCalled());
    const path = dialog.save.mock.calls[dialog.save.mock.calls.length - 1][0].defaultPath as string;
    expect(path.endsWith("/" + want) || path === want).toBe(true);
    expect(path).not.toContain("\\");
    unmount();
    dialog.save.mockClear();
  }
});

test("a recording whose file is gone says so and cannot be played", async () => {
  mockIPC((cmd) => (cmd === "auto_run_recordings" ? [ENTRY] : null));
  show();
  expect(await screen.findByText(VIDEO_GONE)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: `Play ${ENTRY.title}` })).toBeDisabled();
  expect(screen.getByRole("button", { name: `Save ${ENTRY.title}` })).toBeDisabled();
});

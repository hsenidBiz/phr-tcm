// The video kept of a failed case: Play and Download appear only when the
// video is on this machine, Play shows it in the app, Download saves it
// through the save dialog, and a video too large to play can still be saved.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const dialog = vi.hoisted(() => ({ save: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
const toastMock = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn(), info: vi.fn(), warning: vi.fn(), dismiss: vi.fn() }));
vi.mock("../../../src/lib/toast", () => ({ toast: toastMock }));

import CaseVideo, { VIDEO_GONE } from "../../../src/screens/AutoRun/CaseVideo";

const NAME = "video-1700000000000-000001.mp4";

beforeEach(() => {
  URL.createObjectURL = vi.fn(() => "blob:the-video");
  URL.revokeObjectURL = vi.fn();
});

afterEach(() => {
  cleanup();
  clearMocks();
  dialog.save.mockReset();
  for (const m of Object.values(toastMock)) m.mockClear();
});

function show(name: string | null | undefined) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <CaseVideo name={name} />
    </QueryClientProvider>,
  );
}

test("a case with no video shows no controls and asks nothing", () => {
  const asked: string[] = [];
  mockIPC((cmd) => {
    asked.push(cmd);
    return null;
  });
  show(undefined);
  show(null);
  expect(screen.queryByRole("button", { name: /video/i })).toBeNull();
  expect(asked).toEqual([]);
});

test("a video that exists shows Play and Download with its size", async () => {
  mockIPC((cmd) => (cmd === "auto_run_video_info" ? { size: 2048, playable: true, attachable: true } : null));
  show(NAME);
  expect(await screen.findByRole("button", { name: "Play video" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Download video" })).toBeEnabled();
  expect(screen.getByText("2.0 KB")).toBeInTheDocument();
});

test("a video that is gone says so and has no controls", async () => {
  mockIPC((cmd) => (cmd === "auto_run_video_info" ? null : null));
  show(NAME);
  expect(await screen.findByText(VIDEO_GONE)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Play video" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Download video" })).toBeNull();
});

test("Play fetches the video by name and shows it in a video element", async () => {
  const asked: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_video_info") return { size: 9, playable: true, attachable: true };
    if (cmd === "auto_run_video") {
      asked.push(args);
      return btoa("mp4-bytes");
    }
    return null;
  });
  show(NAME);
  fireEvent.click(await screen.findByRole("button", { name: "Play video" }));
  const video = await screen.findByLabelText("Video of the failed case");
  expect(video).toHaveAttribute("src", "blob:the-video");
  expect(asked).toEqual([{ name: NAME }]);
  fireEvent.click(screen.getByRole("button", { name: "Close video" }));
  await waitFor(() => expect(screen.queryByLabelText("Video of the failed case")).toBeNull());
  expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:the-video");
});

test("a video too large to play can still be downloaded", async () => {
  const saved: unknown[] = [];
  dialog.save.mockResolvedValue("C:/out/failure.mp4");
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_video_info") return { size: 80 * 1024 * 1024, playable: false, attachable: false };
    if (cmd === "auto_run_save_video") {
      saved.push(args);
      return null;
    }
    return null;
  });
  show(NAME);
  expect(await screen.findByRole("button", { name: "Play video" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Download video" }));
  await waitFor(() => expect(saved).toEqual([{ name: NAME, destPath: "C:/out/failure.mp4" }]));
  expect(toastMock.success).toHaveBeenCalledWith("Saved the video");
});

test("cancelling the save dialog saves nothing", async () => {
  const saved: unknown[] = [];
  dialog.save.mockResolvedValue(null);
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_video_info") return { size: 9, playable: true, attachable: true };
    if (cmd === "auto_run_save_video") saved.push(args);
    return null;
  });
  show(NAME);
  fireEvent.click(await screen.findByRole("button", { name: "Download video" }));
  await waitFor(() => expect(dialog.save).toHaveBeenCalled());
  expect(saved).toEqual([]);
  expect(toastMock.success).not.toHaveBeenCalled();
});

// The compact variant is the one-line row's: two icon buttons, no second line.
function showCompact(name: string | null | undefined) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <CaseVideo name={name} compact />
    </QueryClientProvider>,
  );
}

test("compact: Watch and Download are icon buttons named for the failure video, with the size in the tooltip", async () => {
  mockIPC((cmd) => (cmd === "auto_run_video_info" ? { size: 2048, playable: true, attachable: true } : null));
  showCompact(NAME);
  const watch = await screen.findByRole("button", { name: "Watch the failure video" });
  expect(watch).toHaveAttribute("title", "Watch the failure video (2.0 KB)");
  expect(screen.getByRole("button", { name: "Download the failure video" })).toHaveAttribute(
    "title",
    "Download the failure video (2.0 KB)",
  );
  // No second line of words: the label and the size are only in the tooltips.
  expect(screen.queryByText("Video of the failure")).toBeNull();
  expect(screen.queryByText("2.0 KB")).toBeNull();
  expect(screen.queryByRole("button", { name: "Play video" })).toBeNull();
});

test("compact: Watch shows the video, and the same button's place then closes it", async () => {
  mockIPC((cmd) => {
    if (cmd === "auto_run_video_info") return { size: 9, playable: true, attachable: true };
    if (cmd === "auto_run_video") return btoa("mp4-bytes");
    return null;
  });
  showCompact(NAME);
  fireEvent.click(await screen.findByRole("button", { name: "Watch the failure video" }));
  expect(await screen.findByLabelText("Video of the failed case")).toHaveAttribute("src", "blob:the-video");
  fireEvent.click(screen.getByRole("button", { name: "Close the failure video" }));
  await waitFor(() => expect(screen.queryByLabelText("Video of the failed case")).toBeNull());
  expect(screen.getByRole("button", { name: "Watch the failure video" })).toBeEnabled();
});

test("compact: a video too large to play can still be downloaded", async () => {
  mockIPC((cmd) => (cmd === "auto_run_video_info" ? { size: 80 * 1024 * 1024, playable: false, attachable: false } : null));
  showCompact(NAME);
  const watch = await screen.findByRole("button", { name: "Watch the failure video" });
  expect(watch).toBeDisabled();
  expect(watch).toHaveAttribute("title", "This video is too large to play here - download it instead");
  expect(screen.getByRole("button", { name: "Download the failure video" })).toBeEnabled();
});

test("compact: a video that is gone leaves the note and both buttons off", async () => {
  mockIPC(() => null);
  showCompact(NAME);
  expect(await screen.findByText(VIDEO_GONE)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Watch the failure video" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Download the failure video" })).toBeDisabled();
});

test("compact: a case with no video shows nothing and asks nothing", () => {
  const asked: string[] = [];
  mockIPC((cmd) => {
    asked.push(cmd);
    return null;
  });
  showCompact(undefined);
  expect(screen.queryByRole("button")).toBeNull();
  expect(asked).toEqual([]);
});

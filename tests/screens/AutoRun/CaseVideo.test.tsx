// The video kept of a failed case: Play and Download appear only when the
// video is on this machine, Play shows it in the app, Download saves it
// through the save dialog, and a video too large to play can still be saved.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const dialog = vi.hoisted(() => ({ save: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
const toastMock = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn(), info: vi.fn(), warning: vi.fn(), dismiss: vi.fn() }));
vi.mock("../../../src/lib/toast", () => ({ toast: toastMock }));

import CaseVideo, { VIDEO_GONE } from "../../../src/screens/AutoRun/CaseVideo";
import { VIDEO_WONT_PLAY } from "../../../src/screens/AutoRun/VideoPlayer";
import type { Timeline } from "../../../src/screens/AutoRun/videoTimeline";

const TIMELINE: Timeline = {
  title: "Video of #7 Leave request",
  length_ms: 20000,
  marks: [],
  steps: [
    { name: "Sign in", at_ms: 0, failed: false, actions: [] },
    { name: "Step 1", at_ms: 4000, failed: true, actions: [{ label: "Click", at_ms: 4200 }] },
  ],
};

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
      <CaseVideo name={name} timeline={TIMELINE} />
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

test("a video that exists shows Watch and Download with its size", async () => {
  mockIPC((cmd) => (cmd === "auto_run_video_info" ? { size: 2048, playable: true, attachable: true } : null));
  show(NAME);
  expect(await screen.findByRole("button", { name: "Watch video" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Download video" })).toBeEnabled();
  expect(screen.getByText("2.0 KB")).toBeInTheDocument();
});

test("a video that is gone says so and has no controls", async () => {
  mockIPC((cmd) => (cmd === "auto_run_video_info" ? null : null));
  show(NAME);
  expect(await screen.findByText(VIDEO_GONE)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Watch video" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Download video" })).toBeNull();
});

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

test("Watch video opens the player over the app with the case's steps", async () => {
  const asked: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_video_info") return { size: 9, playable: true, attachable: true };
    if (cmd === "auto_run_video") {
      asked.push(args);
      return btoa("mp4");
    }
    return null;
  });
  show(NAME);
  fireEvent.click(await screen.findByRole("button", { name: "Watch video" }));
  const dialog = await screen.findByRole("dialog", { name: "Video of #7 Leave request" });
  await waitFor(() => expect(within(dialog).getByLabelText("Video of #7 Leave request")).toHaveAttribute("src", "blob:the-video"));
  expect(within(dialog).getByRole("list", { name: "Steps" })).toBeInTheDocument();
  expect(asked).toEqual([{ name: NAME }]);
});

test("closing the player goes back to the row and lets the video go", async () => {
  mockIPC((cmd) => {
    if (cmd === "auto_run_video_info") return { size: 9, playable: true, attachable: true };
    if (cmd === "auto_run_video") return btoa("mp4");
    return null;
  });
  show(NAME);
  const watchButton = await screen.findByRole("button", { name: "Watch video" });
  watchButton.focus();
  fireEvent.click(watchButton);
  const dialog = await screen.findByRole("dialog");
  await waitFor(() => expect(within(dialog).getByLabelText("Video of #7 Leave request")).toHaveAttribute("src", "blob:the-video"));
  fireEvent.click(screen.getByRole("button", { name: "Close player" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:the-video");
  expect(screen.getByRole("button", { name: "Watch video" })).toHaveFocus();
});

test("closing before the video loads leaves it closed", async () => {
  const gate = deferred<string>();
  mockIPC((cmd) => {
    if (cmd === "auto_run_video_info") return { size: 9, playable: true, attachable: true };
    if (cmd === "auto_run_video") return gate.promise;
    return null;
  });
  show(NAME);
  fireEvent.click(await screen.findByRole("button", { name: "Watch video" }));
  fireEvent.click(await screen.findByRole("button", { name: "Close player" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  gate.resolve(btoa("mp4"));
  await waitFor(() => expect(URL.createObjectURL).toHaveBeenCalled());
  await waitFor(() => expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:the-video"));
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("a video that cannot be fetched opens the player with the will-not-play note", async () => {
  mockIPC((cmd) => {
    if (cmd === "auto_run_video_info") return { size: 9, playable: true, attachable: true };
    if (cmd === "auto_run_video") throw new Error("gone");
    return null;
  });
  show(NAME);
  fireEvent.click(await screen.findByRole("button", { name: "Watch video" }));
  const dialog = await screen.findByRole("dialog", { name: "Video of #7 Leave request" });
  expect(await within(dialog).findByText(VIDEO_WONT_PLAY)).toBeInTheDocument();
  expect(within(dialog).getByRole("list", { name: "Steps" })).toBeInTheDocument();
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
  expect(await screen.findByRole("button", { name: "Watch video" })).toBeDisabled();
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
      <CaseVideo name={name} compact timeline={TIMELINE} />
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
  expect(screen.queryByRole("button", { name: "Watch video" })).toBeNull();
});

test("compact: Watch opens the player and the row stays one line", async () => {
  mockIPC((cmd) => {
    if (cmd === "auto_run_video_info") return { size: 9, playable: true, attachable: true };
    if (cmd === "auto_run_video") return btoa("mp4-bytes");
    return null;
  });
  showCompact(NAME);
  const watchButton = await screen.findByRole("button", { name: "Watch the failure video" });
  watchButton.focus();
  fireEvent.click(watchButton);
  const dialog = await screen.findByRole("dialog", { name: "Video of #7 Leave request" });
  await waitFor(() => expect(within(dialog).getByLabelText("Video of #7 Leave request")).toHaveAttribute("src", "blob:the-video"));
  fireEvent.click(screen.getByRole("button", { name: "Close player" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(screen.getByRole("button", { name: "Watch the failure video" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Watch the failure video" })).toHaveFocus();
});

test("compact: a video too large to play can still be downloaded", async () => {
  mockIPC((cmd) => (cmd === "auto_run_video_info" ? { size: 80 * 1024 * 1024, playable: false, attachable: false } : null));
  showCompact(NAME);
  const watch = await screen.findByRole("button", { name: "Watch the failure video" });
  expect(watch).toBeDisabled();
  expect(watch).toHaveAttribute("title", "This video is too large to play here - download it instead (80.0 MB)");
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

test("a second Watch press while the video is loading does not fetch again", async () => {
  const gate = deferred<string>();
  let fetched = 0;
  mockIPC((cmd) => {
    if (cmd === "auto_run_video_info") return { size: 9, playable: true, attachable: true };
    if (cmd === "auto_run_video") {
      fetched++;
      return gate.promise;
    }
    return null;
  });
  show(NAME);
  const watchButton = await screen.findByRole("button", { name: "Watch video" });
  fireEvent.click(watchButton);
  fireEvent.click(watchButton);
  await waitFor(() => expect(fetched).toBe(1));
  await new Promise((r) => setTimeout(r, 20));
  expect(fetched).toBe(1);
  gate.resolve(btoa("mp4"));
});

test("a failed fetch shows the player's own note and no toast", async () => {
  mockIPC((cmd) => {
    if (cmd === "auto_run_video_info") return { size: 9, playable: true, attachable: true };
    if (cmd === "auto_run_video") throw new Error("gone");
    return null;
  });
  show(NAME);
  fireEvent.click(await screen.findByRole("button", { name: "Watch video" }));
  expect(await screen.findByText(VIDEO_WONT_PLAY)).toBeInTheDocument();
  expect(toastMock.error).not.toHaveBeenCalled();
});

test("watching again while the first load is still running reopens the player with its bytes", async () => {
  const gate = deferred<string>();
  let loads = 0;
  mockIPC((cmd) => {
    if (cmd === "auto_run_video_info") return { size: 9, playable: true, attachable: true };
    if (cmd === "auto_run_video") {
      loads++;
      return gate.promise;
    }
    return null;
  });
  show(NAME);
  fireEvent.click(await screen.findByRole("button", { name: "Watch video" }));
  fireEvent.click(await screen.findByRole("button", { name: "Close player" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  fireEvent.click(screen.getByRole("button", { name: "Watch video" }));
  const dialog = await screen.findByRole("dialog");
  gate.resolve(btoa("mp4"));
  await waitFor(() => expect(within(dialog).getByLabelText("Video of #7 Leave request")).toHaveAttribute("src", "blob:the-video"));
  expect(loads).toBe(1);
  expect(URL.revokeObjectURL).not.toHaveBeenCalled();
});

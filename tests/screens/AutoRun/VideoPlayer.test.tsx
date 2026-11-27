// The video player: a dialog over the app with the video, a bar of step
// ticks, a red failed stretch, the step list and the keys.

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import VideoPlayer, { NO_MARKERS, VIDEO_WONT_PLAY } from "../../../src/screens/AutoRun/VideoPlayer";
import type { Timeline } from "../../../src/screens/AutoRun/videoTimeline";

const TIMELINE: Timeline = {
  title: "Video of #7 Leave request",
  length_ms: 20000,
  marks: [],
  steps: [
    { name: "Sign in", at_ms: 0, failed: false, actions: [] },
    { name: "Step 1", at_ms: 4000, failed: false, actions: [{ label: "Click", at_ms: 4200 }] },
    { name: "Step 2", at_ms: 9000, failed: true, actions: [{ label: "Check: shows 'x'", at_ms: 9100 }] },
  ],
};

const state = { currentTime: 0, playbackRate: 1, duration: Number.NaN };
const play = vi.fn(() => Promise.resolve());
const pause = vi.fn();

beforeEach(() => {
  state.currentTime = 0;
  state.playbackRate = 1;
  state.duration = Number.NaN;
  play.mockClear();
  pause.mockClear();
  const proto = HTMLMediaElement.prototype;
  proto.play = play;
  proto.pause = pause;
  for (const key of ["currentTime", "playbackRate", "duration"] as const) {
    Object.defineProperty(proto, key, {
      configurable: true,
      get: () => state[key],
      set: (v: number) => {
        state[key] = v;
      },
    });
  }
});

afterEach(cleanup);

function show(over: Partial<React.ComponentProps<typeof VideoPlayer>> = {}) {
  const props = {
    timeline: TIMELINE,
    src: "blob:the-video",
    loading: false,
    error: false,
    saving: false,
    onSave: vi.fn(),
    onClose: vi.fn(),
    ...over,
  };
  render(<VideoPlayer {...props} />);
  return { props, video: within(screen.getByRole("dialog")).getByLabelText(props.timeline.title) as HTMLVideoElement };
}

test("a dialog named by the title lists the steps and marks the failed one", () => {
  show();
  const dialog = screen.getByRole("dialog", { name: "Video of #7 Leave request" });
  const list = within(dialog).getByRole("list", { name: "Steps" });
  expect(within(list).getByRole("button", { name: "Step 2 at 0:09, failed" })).toBeInTheDocument();
  expect(within(list).getByRole("button", { name: "Check: shows 'x' at 0:09" })).toBeInTheDocument();
  expect(within(list).queryByRole("button", { name: "Click at 0:04" })).toBeNull();
});

test("positions use length_ms until the video knows its duration", () => {
  const { video } = show();
  expect(screen.getByTestId("failed-stretch").style.left).toBe("45%");
  state.duration = 30;
  fireEvent.loadedMetadata(video);
  expect(screen.getByTestId("failed-stretch").style.left).toBe("30%");
});

test("an infinite duration never puts NaN in a style", () => {
  const { video } = show();
  state.duration = Number.POSITIVE_INFINITY;
  fireEvent.loadedMetadata(video);
  expect(screen.getByTestId("failed-stretch").style.left).toBe("45%");
});

test("a mark later than the video's end stays on the bar", () => {
  show({ timeline: { ...TIMELINE, length_ms: 5000 } });
  expect(screen.getByTestId("failed-stretch").style.left).toBe("100%");
});

test("each step has a tick titled with its name", () => {
  show();
  const slider = screen.getByRole("slider", { name: "Position" });
  for (const name of ["Sign in", "Step 1", "Step 2"]) {
    expect(slider.querySelectorAll(`[title="${name}"]`)).toHaveLength(1);
  }
});

test("clicking a step or an action jumps the video there", () => {
  const { video } = show();
  fireEvent.click(screen.getByRole("button", { name: "Step 1 at 0:04" }));
  expect(video.currentTime).toBe(4);
  fireEvent.click(screen.getByRole("button", { name: "Actions of Step 1" }));
  expect(screen.getByRole("button", { name: "Actions of Step 1" })).toHaveAttribute("aria-expanded", "true");
  fireEvent.click(screen.getByRole("button", { name: "Click at 0:04" }));
  expect(video.currentTime).toBe(4.2);
});

test("a jump clamps to the video's length", () => {
  const { video } = show({ timeline: { ...TIMELINE, length_ms: 8000 } });
  fireEvent.click(screen.getByRole("button", { name: "Step 2 at 0:09, failed" }));
  expect(video.currentTime).toBe(8);
});

test("Space plays and pauses, arrows skip 5 s, [ and ] move between steps", () => {
  const { video } = show();
  const dialog = screen.getByRole("dialog");
  fireEvent.keyDown(dialog, { key: " " });
  expect(play).toHaveBeenCalledTimes(1);
  fireEvent.keyDown(dialog, { key: " " });
  expect(pause).toHaveBeenCalledTimes(1);
  video.currentTime = 4;
  fireEvent.keyDown(dialog, { key: "ArrowRight" });
  expect(video.currentTime).toBe(9);
  fireEvent.keyDown(dialog, { key: "ArrowLeft" });
  expect(video.currentTime).toBe(4);
  fireEvent.keyDown(dialog, { key: "]" });
  expect(video.currentTime).toBe(9);
  video.currentTime = 9.2;
  fireEvent.keyDown(dialog, { key: "[" });
  expect(video.currentTime).toBe(4);
});

test("Space on a focused button presses it and does not toggle play", () => {
  show();
  const step = screen.getByRole("button", { name: "Step 1 at 0:04" });
  step.focus();
  fireEvent.keyDown(step, { key: " " });
  expect(play).not.toHaveBeenCalled();
});

test("the playing step is marked as the video plays", () => {
  const { video } = show();
  video.currentTime = 9.5;
  fireEvent.timeUpdate(video);
  expect(screen.getByRole("button", { name: "Step 2 at 0:09, failed" })).toHaveAttribute("aria-current", "step");
  expect(screen.getByRole("button", { name: "Step 1 at 0:04" })).not.toHaveAttribute("aria-current");
  expect(screen.getByRole("slider", { name: "Position" })).toHaveAttribute("aria-valuetext", "0:09 of 0:20, Step 2");
});

test("speed buttons set the playback rate", () => {
  const { video } = show();
  const group = screen.getByRole("group", { name: "Speed" });
  fireEvent.click(within(group).getByRole("button", { name: "2x" }));
  expect(video.playbackRate).toBe(2);
  expect(within(group).getByRole("button", { name: "2x" })).toHaveAttribute("aria-pressed", "true");
  expect(within(group).getByRole("button", { name: "1x" })).toHaveAttribute("aria-pressed", "false");
});

test("Save video calls onSave; Close player and Escape call onClose", () => {
  const { props } = show();
  fireEvent.click(screen.getByRole("button", { name: "Save video" }));
  expect(props.onSave).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Close player" }));
  expect(props.onClose).toHaveBeenCalledTimes(1);
  fireEvent.keyDown(window, { key: "Escape" });
  expect(props.onClose).toHaveBeenCalledTimes(2);
});

test("Save video is off while saving", () => {
  show({ saving: true });
  expect(screen.getByRole("button", { name: "Save video" })).toBeDisabled();
});

test("a recording gets a flat list of actions", () => {
  show({
    timeline: { title: "Recording: Bug 1", length_ms: 94000, steps: [], marks: [{ label: "Click", at_ms: 3000 }] },
  });
  const list = screen.getByRole("list", { name: "Actions" });
  expect(within(list).getByRole("button", { name: "Click at 0:03" })).toBeInTheDocument();
  expect(screen.queryByRole("list", { name: "Steps" })).toBeNull();
  expect(screen.queryByTestId("failed-stretch")).toBeNull();
});

test("an old video opens without markers", () => {
  show({ timeline: { title: "Video of #7 Leave request", length_ms: null, steps: [], marks: [] } });
  expect(screen.getByText(NO_MARKERS)).toBeInTheDocument();
  expect(screen.queryByRole("list")).toBeNull();
  expect(screen.queryByTestId("failed-stretch")).toBeNull();
});

test("a video that will not play says so and keeps the list", () => {
  show({ error: true });
  expect(screen.getByText(VIDEO_WONT_PLAY)).toBeInTheDocument();
  expect(screen.getByRole("list", { name: "Steps" })).toBeInTheDocument();
});

test("a video element error says the same", () => {
  const { video } = show();
  fireEvent.error(video);
  expect(screen.getByText(VIDEO_WONT_PLAY)).toBeInTheDocument();
});

// The person's Allow or Deny before the assistant replays a must-not-save
// script: shown when the app hears the request, answered through the
// command, and closed by the answer or by the request ending.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../../lib/toast";
import ReplayRequestModal from "./ReplayRequestModal";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));

afterEach(() => {
  cleanup();
  clearMocks();
  vi.clearAllMocks();
});

const REQUEST = { id: "replay-1-ab", case_id: 77, title: "Leave request", step: 3 };
const TEXT =
  "The assistant wants to replay case 77 (Leave request) up to step 3. This script must not save; the guard stays on. Allow?";

function mount(answer: (args: Record<string, unknown>) => unknown = () => null) {
  const answers: Record<string, unknown>[] = [];
  mockIPC(
    (cmd, args) => {
      if (cmd === "auto_run_answer_replay_request") {
        answers.push((args ?? {}) as Record<string, unknown>);
        return answer((args ?? {}) as Record<string, unknown>);
      }
      return null;
    },
    { shouldMockEvents: true },
  );
  render(<ReplayRequestModal />);
  return answers;
}

async function send(event: string, payload: unknown) {
  const { emit } = await import("@tauri-apps/api/event");
  await act(async () => {
    await emit(event, payload);
  });
}

test("nothing shows until the assistant asks", () => {
  mount();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("the request shows the exact text, and Allow answers it and closes", async () => {
  const answers = mount();
  await send("autorun-replay-request", REQUEST);
  expect(await screen.findByText(TEXT)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Allow" }));
  await waitFor(() => expect(answers).toEqual([{ id: "replay-1-ab", allow: true }]));
  await waitFor(() => expect(screen.queryByText(TEXT)).not.toBeInTheDocument());
});

test("Deny answers it with allow false and closes", async () => {
  const answers = mount();
  await send("autorun-replay-request", REQUEST);
  fireEvent.click(await screen.findByRole("button", { name: "Deny" }));
  await waitFor(() => expect(answers).toEqual([{ id: "replay-1-ab", allow: false }]));
  await waitFor(() => expect(screen.queryByText(TEXT)).not.toBeInTheDocument());
});

test("a request that stops waiting closes the prompt; another request's end does not", async () => {
  mount();
  await send("autorun-replay-request", REQUEST);
  expect(await screen.findByText(TEXT)).toBeInTheDocument();
  await send("autorun-replay-request-ended", { id: "replay-0-other" });
  expect(screen.getByText(TEXT)).toBeInTheDocument();
  await send("autorun-replay-request-ended", { id: "replay-1-ab" });
  await waitFor(() => expect(screen.queryByText(TEXT)).not.toBeInTheDocument());
});

test("a late answer closes the prompt and says the request is no longer waiting", async () => {
  mount(() => {
    throw "that replay request is no longer waiting";
  });
  await send("autorun-replay-request", REQUEST);
  fireEvent.click(await screen.findByRole("button", { name: "Allow" }));
  await waitFor(() => expect(toast.warning).toHaveBeenCalledWith("that replay request is no longer waiting"));
  await waitFor(() => expect(screen.queryByText(TEXT)).not.toBeInTheDocument());
});

test("Escape while an answer is in flight sends nothing more", async () => {
  let release: () => void = () => {};
  const held = new Promise<null>((resolve) => {
    release = () => resolve(null);
  });
  const answers = mount(() => held);
  await send("autorun-replay-request", REQUEST);
  fireEvent.click(await screen.findByRole("button", { name: "Allow" }));
  await waitFor(() => expect(answers).toHaveLength(1));
  fireEvent.keyDown(window, { key: "Escape" });
  await act(async () => {
    release();
    await held;
  });
  await waitFor(() => expect(screen.queryByText(TEXT)).not.toBeInTheDocument());
  expect(answers).toEqual([{ id: "replay-1-ab", allow: true }]);
});

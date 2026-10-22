import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import PageSizeSettings from "./PageSizeSettings";

vi.mock("../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const REFUSAL = "The page size must be between 800 x 600 and 3840 x 2160.";
const HELP =
  "Every browser the app drives shows its page at this size. It applies to the next browser the app opens. Default 1280 x 720.";

type Size = { width: number; height: number };

/** Rust's side: the saved size, and the three commands that touch it. */
function rust(start: Size) {
  const calls: { cmd: string; args?: unknown }[] = [];
  let page_size = start;
  mockIPC((cmd, args) => {
    if (cmd === "get_app_settings") return { close_to_tray: true, close_notice_shown: true, beta_updates: false, page_size };
    if (cmd === "set_page_size") {
      calls.push({ cmd, args });
      const size = (args as { size: Size }).size;
      if (size.width < 800 || size.width > 3840 || size.height < 600 || size.height > 2160) throw REFUSAL;
      page_size = size;
      return size;
    }
    if (cmd === "reset_page_size") {
      calls.push({ cmd });
      page_size = { width: 1280, height: 720 };
      return page_size;
    }
    return undefined;
  });
  return calls;
}

function renderRow() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <PageSizeSettings />
    </QueryClientProvider>,
  );
}

const width = () => screen.getByRole("spinbutton", { name: "Width" }) as HTMLInputElement;
const height = () => screen.getByRole("spinbutton", { name: "Height" }) as HTMLInputElement;

test("the row names itself, explains itself and labels both boxes", async () => {
  rust({ width: 1280, height: 720 });
  renderRow();
  expect(screen.getByText("Browser page size")).toBeInTheDocument();
  expect(screen.getByText(HELP)).toBeInTheDocument();
  expect(width()).toBeInTheDocument();
  expect(height()).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Save" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Reset to default: page size" })).toBeInTheDocument();
  await waitFor(() => expect(width()).not.toBeDisabled());
});

test("it shows the size Rust has saved", async () => {
  rust({ width: 1920, height: 1080 });
  renderRow();
  await waitFor(() => expect(width()).toHaveValue(1920));
  expect(height()).toHaveValue(1080);
  // Nothing edited: nothing to save.
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
});

test("Save sends the typed size to set_page_size and keeps what Rust answers", async () => {
  const calls = rust({ width: 1280, height: 720 });
  renderRow();
  await waitFor(() => expect(width()).not.toBeDisabled());
  fireEvent.change(width(), { target: { value: "1600" } });
  fireEvent.change(height(), { target: { value: "900" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(calls).toEqual([{ cmd: "set_page_size", args: { size: { width: 1600, height: 900 } } }]));
  await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeDisabled());
  expect(width()).toHaveValue(1600);
  expect(height()).toHaveValue(900);
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("a refused size shows Rust's sentence inline and the boxes keep what was typed", async () => {
  const calls = rust({ width: 1280, height: 720 });
  renderRow();
  await waitFor(() => expect(width()).not.toBeDisabled());
  fireEvent.change(width(), { target: { value: "799" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(REFUSAL);
  expect(calls).toHaveLength(1);
  expect(width()).toHaveValue(799);
  // A later good save clears it.
  fireEvent.change(width(), { target: { value: "1024" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
});

test("Reset to default calls reset_page_size and shows the shipped size", async () => {
  const calls = rust({ width: 1920, height: 1080 });
  renderRow();
  await waitFor(() => expect(width()).toHaveValue(1920));
  fireEvent.click(screen.getByRole("button", { name: "Reset to default: page size" }));
  await waitFor(() => expect(calls).toEqual([{ cmd: "reset_page_size" }]));
  await waitFor(() => expect(width()).toHaveValue(1280));
  expect(height()).toHaveValue(720);
});

test("Reset has nothing to do while the shipped size is saved and untouched", async () => {
  rust({ width: 1280, height: 720 });
  renderRow();
  await waitFor(() => expect(width()).not.toBeDisabled());
  expect(screen.getByRole("button", { name: "Reset to default: page size" })).toBeDisabled();
  fireEvent.change(height(), { target: { value: "800" } });
  expect(screen.getByRole("button", { name: "Reset to default: page size" })).not.toBeDisabled();
});

test("a decimal, a negative, an exponent or an empty box shows the refusal sentence and sends nothing", async () => {
  const calls = rust({ width: 1280, height: 720 });
  renderRow();
  await waitFor(() => expect(width()).not.toBeDisabled());
  for (const [w, h] of [
    ["1280.5", "720"],
    ["1280", "-5"],
    ["-1280", "720"],
    ["", "720"],
    ["1280", ""],
    ["1e3", "720"],
  ]) {
    fireEvent.change(width(), { target: { value: w } });
    fireEvent.change(height(), { target: { value: h } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(REFUSAL);
    expect(screen.getByRole("alert")).not.toHaveTextContent(/invalid args|expected u32/i);
  }
  expect(calls).toEqual([]);
  // A good size afterwards goes through and clears the sentence.
  fireEvent.change(width(), { target: { value: "1024" } });
  fireEvent.change(height(), { target: { value: "768" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(calls).toHaveLength(1));
  await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
});

test("a whole number outside the range still goes to Rust, which refuses it in its own words", async () => {
  const calls = rust({ width: 1280, height: 720 });
  renderRow();
  await waitFor(() => expect(width()).not.toBeDisabled());
  fireEvent.change(width(), { target: { value: "99999" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(REFUSAL);
  expect(calls).toHaveLength(1);
});

test("the reset button's accessible name holds its visible words", async () => {
  rust({ width: 1920, height: 1080 });
  renderRow();
  await waitFor(() => expect(width()).toHaveValue(1920));
  const reset = screen.getByRole("button", { name: "Reset to default: page size" });
  // WCAG 2.5.3: speaking the visible words must reach the button.
  expect(reset.getAttribute("aria-label")).toContain(reset.textContent ?? "never");
  // And it is not the name another Reset to default button has.
  expect(screen.queryByRole("button", { name: "Reset to default" })).not.toBeInTheDocument();
});

test("a reset that could not be saved says so inline, like a failed save, and keeps the boxes", async () => {
  const SAVE_FAILED = "The setting could not be saved. Settings → Logs has the details.";
  mockIPC((cmd) => {
    if (cmd === "get_app_settings") {
      return { close_to_tray: true, close_notice_shown: true, beta_updates: false, page_size: { width: 1920, height: 1080 } };
    }
    if (cmd === "reset_page_size") throw SAVE_FAILED;
    return undefined;
  });
  renderRow();
  await waitFor(() => expect(width()).toHaveValue(1920));
  fireEvent.click(screen.getByRole("button", { name: "Reset to default: page size" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(SAVE_FAILED);
  expect(width()).toHaveValue(1920);
  expect(height()).toHaveValue(1080);
});

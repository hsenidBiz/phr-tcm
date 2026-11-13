import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { ResultAttachment } from "../bindings";
import { downloadResultFile, downloadSessionFile } from "./runnerFiles";

const toastMock = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));
vi.mock("./toast", () => ({ toast: toastMock }));

const dialog = vi.hoisted(() => ({ save: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);

let calls: Array<{ cmd: string; args: Record<string, unknown> }> = [];
let respond: (cmd: string) => unknown = () => null;

beforeEach(() => {
  calls = [];
  respond = () => null;
  toastMock.success.mockClear();
  toastMock.error.mockClear();
  dialog.save.mockReset();
  localStorage.clear();
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    return respond(cmd);
  });
});
afterEach(() => clearMocks());

const att: ResultAttachment = { id: 9, file_name: "rec.webm", size_bytes: 5000000, kind: "video", thumb_b64: null };

test("cancelling the save dialog writes nothing", async () => {
  dialog.save.mockResolvedValue(null);
  expect(await downloadSessionFile({ file_name: "a.txt", b64: "QQ==" })).toBe(false);
  expect(calls.some((c) => c.cmd === "save_attachment_b64")).toBe(false);
  expect(toastMock.success).not.toHaveBeenCalled();
});

test("a session file is saved where the person chose", async () => {
  dialog.save.mockResolvedValue("C:/out/a.txt");
  expect(await downloadSessionFile({ file_name: "a.txt", b64: "QQ==" })).toBe(true);
  const c = calls.find((x) => x.cmd === "save_attachment_b64")!;
  expect(c.args).toEqual({ destPath: "C:/out/a.txt", b64: "QQ==" });
  expect(toastMock.success).toHaveBeenCalledWith("Saved a.txt");
});

test("an uploaded file is fetched by Rust, not read into the webview", async () => {
  dialog.save.mockResolvedValue("C:/out/rec.webm");
  const atob = vi.spyOn(globalThis, "atob");
  expect(await downloadResultFile("org", "proj", 3, 30, att)).toBe(true);
  const c = calls.find((x) => x.cmd === "save_result_attachment")!;
  expect(c.args).toMatchObject({ attachmentId: 9, sizeBytes: 5000000, destPath: "C:/out/rec.webm", runId: 3, resultId: 30 });
  expect(calls.map((x) => x.cmd)).not.toContain("read_file_b64");
  expect(atob).not.toHaveBeenCalled();
  atob.mockRestore();
});

test("a failure toasts the reason", async () => {
  dialog.save.mockResolvedValue("C:/out/rec.webm");
  respond = (cmd) => {
    if (cmd === "save_result_attachment") throw { kind: "Forbidden" };
    return null;
  };
  expect(await downloadResultFile("org", "proj", 3, 30, att)).toBe(false);
  expect(toastMock.error).toHaveBeenCalledTimes(1);
  expect(String(toastMock.error.mock.calls[0][0])).toBe("Could not save rec.webm: You don't have permission for this resource.");
  expect(toastMock.success).not.toHaveBeenCalled();
});

test("a session file failure toasts Rust's sentence", async () => {
  dialog.save.mockResolvedValue("C:/out/a.txt");
  respond = (cmd) => {
    if (cmd === "save_attachment_b64") throw "The destination is not writable.";
    return null;
  };
  expect(await downloadSessionFile({ file_name: "a.txt", b64: "QQ==" })).toBe(false);
  expect(String(toastMock.error.mock.calls[0][0])).toBe("Could not save a.txt: The destination is not writable.");
});

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, expect, test } from "vitest";
import { commands } from "./bindings";

afterEach(() => clearMocks());

test("ping round-trips through the typed bindings", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "ping") return `pong: ${(args as { msg: string }).msg}`;
  });
  const result = await commands.ping("hello");
  expect(result).toBe("pong: hello");
});

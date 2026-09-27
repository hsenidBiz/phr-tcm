import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { expect, test } from "vitest";
import BackgroundSettings from "./BackgroundSettings";

function renderIt() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <BackgroundSettings />
    </QueryClientProvider>,
  );
}

test("shows the saved choices and changes them", async () => {
  const calls: string[] = [];
  let tray = true;
  let autostart = false;
  mockIPC((cmd, args) => {
    const a = args as { on?: boolean };
    if (cmd === "get_app_settings") return { close_to_tray: tray, close_notice_shown: false, beta_updates: false };
    if (cmd === "set_close_to_tray") {
      calls.push(`tray:${a.on}`);
      tray = !!a.on;
      return { close_to_tray: tray, close_notice_shown: false, beta_updates: false };
    }
    if (cmd === "get_autostart") return autostart;
    if (cmd === "set_autostart") {
      calls.push(`autostart:${a.on}`);
      autostart = !!a.on;
      return autostart;
    }
  });
  renderIt();
  const traySwitch = await screen.findByRole("switch", { name: "Keep running in the tray when closed" });
  const startSwitch = screen.getByRole("switch", { name: "Start with Windows" });
  await waitFor(() => expect(traySwitch).toHaveAttribute("aria-checked", "true"));
  expect(startSwitch).toHaveAttribute("aria-checked", "false");

  fireEvent.click(traySwitch);
  await waitFor(() => expect(traySwitch).toHaveAttribute("aria-checked", "false"));
  fireEvent.click(startSwitch);
  await waitFor(() => expect(startSwitch).toHaveAttribute("aria-checked", "true"));
  expect(calls).toEqual(["tray:false", "autostart:true"]);
});

test("a change that fails puts the switch back", async () => {
  mockIPC((cmd) => {
    if (cmd === "get_app_settings") return { close_to_tray: true, close_notice_shown: false, beta_updates: false };
    if (cmd === "set_close_to_tray") throw "The setting could not be saved. Settings → Logs has the details.";
    if (cmd === "get_autostart") return false;
    if (cmd === "set_autostart") throw "Start with Windows could not be changed. Settings → Logs has the details.";
  });
  renderIt();
  const traySwitch = await screen.findByRole("switch", { name: "Keep running in the tray when closed" });
  await waitFor(() => expect(traySwitch).toHaveAttribute("aria-checked", "true"));
  fireEvent.click(traySwitch);
  await waitFor(() => expect(traySwitch).toHaveAttribute("aria-checked", "true"));
  const startSwitch = screen.getByRole("switch", { name: "Start with Windows" });
  fireEvent.click(startSwitch);
  await waitFor(() => expect(startSwitch).toHaveAttribute("aria-checked", "false"));
});

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

test("Start minimized follows the setting, and only works while Start with Windows is on", async () => {
  const calls: string[] = [];
  let autostart = false;
  let minimized = true;
  mockIPC((cmd, args) => {
    const a = args as { on?: boolean };
    const settings = () => ({ close_to_tray: true, close_notice_shown: false, beta_updates: false, start_minimized: minimized });
    if (cmd === "get_app_settings") return settings();
    if (cmd === "get_autostart") return autostart;
    if (cmd === "set_autostart") {
      autostart = !!a.on;
      return autostart;
    }
    if (cmd === "set_start_minimized") {
      calls.push(`minimized:${a.on}`);
      minimized = !!a.on;
      return settings();
    }
  });
  renderIt();
  const minimizedSwitch = await screen.findByRole("switch", { name: "Start minimized" });
  await waitFor(() => expect(minimizedSwitch).toHaveAttribute("aria-checked", "true"));
  // Start with Windows is off: nothing to minimize yet.
  expect(minimizedSwitch).toBeDisabled();

  fireEvent.click(screen.getByRole("switch", { name: "Start with Windows" }));
  await waitFor(() => expect(minimizedSwitch).not.toBeDisabled());
  fireEvent.click(minimizedSwitch);
  await waitFor(() => expect(minimizedSwitch).toHaveAttribute("aria-checked", "false"));
  expect(calls).toEqual(["minimized:false"]);
});

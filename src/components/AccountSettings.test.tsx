import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { expect, test } from "vitest";
import AccountSettings from "./AccountSettings";

const settings = (stay: boolean) => ({
  close_to_tray: true,
  close_notice_shown: false,
  beta_updates: false,
  start_minimized: true,
  stay_signed_in: stay,
});

function renderIt() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <AccountSettings />
    </QueryClientProvider>,
  );
  return qc;
}

test("names who is signed in and changes Stay signed in", async () => {
  const calls: string[] = [];
  let stay = true;
  mockIPC((cmd, args) => {
    const a = args as { on?: boolean };
    if (cmd === "resume_session") return { signed_in: true, account: "avin@example.com" };
    if (cmd === "get_app_settings") return settings(stay);
    if (cmd === "set_stay_signed_in") {
      calls.push(`stay:${a.on}`);
      stay = !!a.on;
      return settings(stay);
    }
  });
  renderIt();
  expect(await screen.findByText("Signed in as avin@example.com")).toBeInTheDocument();
  const staySwitch = screen.getByRole("switch", { name: "Stay signed in" });
  await waitFor(() => expect(staySwitch).toHaveAttribute("aria-checked", "true"));
  fireEvent.click(staySwitch);
  await waitFor(() => expect(staySwitch).toHaveAttribute("aria-checked", "false"));
  expect(calls).toEqual(["stay:false"]);
});

test("a Stay signed in change that fails puts the switch back", async () => {
  mockIPC((cmd) => {
    if (cmd === "resume_session") return { signed_in: true, account: "a@b.com" };
    if (cmd === "get_app_settings") return settings(true);
    if (cmd === "set_stay_signed_in") throw "The setting could not be saved. Settings → Logs has the details.";
  });
  renderIt();
  const staySwitch = await screen.findByRole("switch", { name: "Stay signed in" });
  await waitFor(() => expect(staySwitch).toHaveAttribute("aria-checked", "true"));
  fireEvent.click(staySwitch);
  await waitFor(() => expect(staySwitch).toHaveAttribute("aria-checked", "true"));
});

test("Sign out signs out, and drops the previous person's data but not the app's", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    if (cmd === "resume_session") return { signed_in: true, account: "a@b.com" };
    if (cmd === "get_app_settings") return settings(true);
    if (cmd === "sign_out") {
      calls.push("sign_out");
      return { signed_in: false, account: null };
    }
  });
  const qc = renderIt();
  qc.setQueryData(["pbi-cases", "org", "proj", 1], ["someone's cases"]);
  qc.setQueryData(["update"], { available: null });
  fireEvent.click(await screen.findByRole("button", { name: "Sign out" }));
  await waitFor(() => expect(qc.getQueryData(["auth"])).toEqual({ signed_in: false, account: null }));
  expect(calls).toEqual(["sign_out"]);
  expect(qc.getQueryData(["pbi-cases", "org", "proj", 1])).toBeUndefined();
  expect(qc.getQueryData(["update"])).toEqual({ available: null });
});

test("a Sign out whose kept copy could not be removed still signs out, and says so", async () => {
  mockIPC((cmd) => {
    if (cmd === "resume_session") return { signed_in: true, account: "a@b.com" };
    if (cmd === "get_app_settings") return settings(true);
    if (cmd === "sign_out") throw "Signed out, but the kept sign-in could not be removed from Windows Credential Manager.";
  });
  const qc = renderIt();
  fireEvent.click(await screen.findByRole("button", { name: "Sign out" }));
  await waitFor(() => expect(qc.getQueryData(["auth"])).toEqual({ signed_in: false, account: null }));
});

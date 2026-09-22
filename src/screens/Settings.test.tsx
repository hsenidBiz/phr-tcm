import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import Settings from "./Settings";
import { CHANGELOG } from "../lib/changelog";

afterEach(() => clearMocks());

function renderSettings(qc: QueryClient) {
  return render(
    <QueryClientProvider client={qc}>
      <Settings org="acme" project="Web" />
    </QueryClientProvider>,
  );
}

/// Reporting a bug is one click from the gear: the button sits in the
/// Changelog header, not behind the Logs panel.
test("Report a bug opens its dialog straight from the Changelog view", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  expect(screen.getByRole("button", { name: "Changelog", pressed: true })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Report a bug" }));
  expect(await screen.findByText("Report a bug in this app")).toBeInTheDocument();
});

test("manual update check seeds the [\"update\"] query the App banner reads", async () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  mockIPC((cmd) => {
    if (cmd === "check_update") return "9.9.9";
    if (cmd === "plugin:app|version") return "1.6.0";
  });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: /Check for updates/ }));
  // The banner in App renders from this cache entry (fetched once at
  // startup with staleTime Infinity) - a manual check must write it too.
  await waitFor(() => expect(qc.getQueryData(["update"])).toBe("9.9.9"));
});

test("up-to-date check clears any stale banner state", async () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  qc.setQueryData(["update"], "0.0.1"); // pretend a stale value
  mockIPC((cmd) => {
    if (cmd === "check_update") return null;
    if (cmd === "plugin:app|version") return "1.6.0";
  });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: /Check for updates/ }));
  await waitFor(() => expect(qc.getQueryData(["update"])).toBeNull());
});

/// The changelog used to fill the right column. It now opens on the latest
/// version, with the rest behind Show more.
test("the changelog shows the latest version, and Show more unfolds the history", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  expect(await screen.findByRole("heading", { name: "Changelog" })).toBeInTheDocument();
  // No line of description under the title - the heading says it.
  expect(screen.queryByText(/the same notes the post-update popup shows/)).not.toBeInTheDocument();
  expect(screen.getByText(`Version ${CHANGELOG[0].version}`)).toBeInTheDocument();
  expect(screen.queryByText("Version 1.9.0")).not.toBeInTheDocument();

  const more = screen.getByRole("button", { name: `Show more (${CHANGELOG.length - 1} earlier versions)` });
  expect(more).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(more);
  expect(screen.getByText("Version 1.9.0")).toBeInTheDocument();
  expect(screen.getByText("Version 1.7.1")).toBeInTheDocument();

  const less = screen.getByRole("button", { name: "Show less" });
  expect(less).toHaveAttribute("aria-expanded", "true");
  fireEvent.click(less);
  expect(screen.queryByText("Version 1.9.0")).not.toBeInTheDocument();
});

/// The settings that used to sit alone at the bottom of the left column now
/// sit under the changelog, in the right column.
test("Backup and Updates sit under the changelog, in the same column", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  const changelog = await screen.findByRole("heading", { name: "Changelog" });
  const column = changelog.closest("section")!.parentElement!;
  const headings = within(column)
    .getAllByRole("heading", { level: 2 })
    .map((h) => h.textContent);
  expect(headings).toEqual(["Changelog", "Backup & transfer", "Updates"]);
  // And the left column keeps the rest, without them.
  const appearance = screen.getByRole("heading", { name: "Appearance" }).closest("section")!.parentElement!;
  expect(within(appearance).queryByRole("heading", { name: "Updates" })).not.toBeInTheDocument();
  expect(appearance).not.toBe(column);
});

/// Machine-wide registration is an explicit opt-in, and this switch is the
/// only place it is granted - the AI Bridge tab reads the same key.
test("the machine-wide AI registration switch persists its choice", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  const sw = await screen.findByLabelText("Allow registering AI tools machine-wide");
  expect(localStorage.getItem("tcm-v2-ai-global-allowed")).toBeNull();
  fireEvent.click(sw);
  expect(localStorage.getItem("tcm-v2-ai-global-allowed")).toBe("on");
  fireEvent.click(sw);
  expect(localStorage.getItem("tcm-v2-ai-global-allowed")).toBeNull();
  localStorage.clear();
});

test("Settings carries no AI Bridge content (it lives in its own tab)", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  await screen.findByRole("heading", { name: "Changelog" });
  // The changelog history may mention "AI Bridge" in release notes - assert
  // the section itself and the old moved-note are gone, not the words.
  expect(screen.queryByText("AI Bridge has moved to its own tab.")).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "AI Bridge" })).not.toBeInTheDocument();
  expect(screen.queryByText("Registered in Claude Code:")).not.toBeInTheDocument();
});

test("the right column switches from the changelog to the app log", async () => {
  mockIPC((cmd) => {
    if (cmd === "app_logs")
      return [
        { at: "2026-07-26 09:00:01", level: "info", message: "Test Case Manager started" },
        { at: "2026-07-26 09:02:20", level: "error", message: "Submit failed for 'X'" },
      ];
    if (cmd === "app_log_dir") return "C:\logs";
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);

  // Changelog is the default panel.
  expect(await screen.findByRole("button", { name: /Show more/ })).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Logs" }));
  expect(await screen.findByText("Test Case Manager started")).toBeInTheDocument();
  expect(screen.getByText("Submit failed for 'X'")).toBeInTheDocument();
  // The changelog panel is gone, not merely hidden below.
  expect(screen.queryByRole("button", { name: /Show more/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Copy log" })).toBeInTheDocument();
});

/// The folder opens from Rust. The frontend used to call the opener plugin
/// itself, and the webview's `opener:default` permission does not include
/// open_path - so the plugin refused and the button only ever showed the
/// error toast.
test("Open log folder asks Rust to open it", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(String(cmd));
    if (cmd === "app_logs") return [];
    if (cmd === "app_log_dir") return "C:\\logs";
    if (cmd === "open_app_log_dir") return { status: "ok", data: null };
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: "Logs" }));
  const open = await screen.findByRole("button", { name: "Open log folder" });
  await waitFor(() => expect(open).toBeEnabled());
  fireEvent.click(open);
  await waitFor(() => expect(calls).toContain("open_app_log_dir"));
  expect(calls.some((c) => c.startsWith("plugin:opener|"))).toBe(false);
});

// Default tags are no longer set here - they moved to Manual Entry, where
// they are used, and their tests went with them (ManualEntry.test.tsx).
test("default tags are not offered in Settings any more", () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    return undefined;
  });
  renderSettings(qc);
  expect(screen.queryByLabelText("Default tags")).not.toBeInTheDocument();
});

test("export sends only the app's own localStorage keys to the backend", async () => {
  localStorage.setItem("tcm-v2-theme", "dark");
  localStorage.setItem("someone-elses-key", "x");
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  let sent: Record<string, string> | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "plugin:dialog|save") return "C:/tmp/tcm-backup.json";
    if (cmd === "export_app_backup") {
      sent = (args as { localStorage: Record<string, string> }).localStorage;
      return { path: "C:/tmp/tcm-backup.json", keys: 1, files: 3, skipped: [] };
    }
    return undefined;
  });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: "Export to file" }));
  await waitFor(() => expect(sent).not.toBeNull());
  expect(sent).toEqual({ "tcm-v2-theme": "dark" });
  localStorage.clear();
});

test("import asks for confirmation, applies the backup's keys, and reloads", async () => {
  localStorage.setItem("tcm-v2-stale", "gone-after-import");
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  mockIPC((cmd) => {
    if (cmd === "plugin:dialog|open") return "C:/tmp/tcm-backup.json";
    if (cmd === "import_app_backup")
      return {
        local_storage: { "tcm-v2-theme": "dark" },
        files_restored: 2,
        exported_at: "2026-08-19 10:00:00",
        app_version: "1.20.4",
      };
    return undefined;
  });
  // jsdom's location.reload is not writable directly - swap the object.
  const original = window.location;
  const reload = vi.fn();
  Object.defineProperty(window, "location", {
    configurable: true,
    value: { ...original, reload },
  });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: "Import from file" }));
  // Nothing is touched until the modal's confirm - the dialog pick alone
  // must not import.
  expect(await screen.findByRole("dialog")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-theme")).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Import and reload" }));
  await waitFor(() => expect(reload).toHaveBeenCalled());
  expect(localStorage.getItem("tcm-v2-theme")).toBe("dark");
  // Replace semantics: a key the backup doesn't carry is removed.
  expect(localStorage.getItem("tcm-v2-stale")).toBeNull();

  Object.defineProperty(window, "location", { configurable: true, value: original });
  localStorage.clear();
});

test("cancelling the import confirmation touches nothing", async () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  let imported = false;
  mockIPC((cmd) => {
    if (cmd === "plugin:dialog|open") return "C:/tmp/tcm-backup.json";
    if (cmd === "import_app_backup") {
      imported = true;
      return { local_storage: {}, files_restored: 0, exported_at: "", app_version: "" };
    }
    return undefined;
  });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: "Import from file" }));
  await screen.findByRole("dialog");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(imported).toBe(false);
});

test("the PHR-X card switch persists its choice, on by default", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  const sw = await screen.findByLabelText("Show the company database section on the AI Bridge tab");
  expect(sw).toBeChecked();
  expect(localStorage.getItem("tcm-v2-ai-show-db")).toBeNull();
  fireEvent.click(sw);
  expect(localStorage.getItem("tcm-v2-ai-show-db")).toBe("off");
  fireEvent.click(sw);
  expect(localStorage.getItem("tcm-v2-ai-show-db")).toBeNull();
  localStorage.clear();
});

/// The tour's last three stops are rung on this screen, so the sections it
/// names have to keep their `data-tour` attributes. `tourAnchors.test.ts`
/// only proves the names exist SOMEWHERE in src/; this proves they are on
/// the right sections here, with something in them to ring.
test("the tour's three Settings anchors sit on the sections it names", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { container } = renderSettings(qc);
  await screen.findByRole("button", { name: "Changelog", pressed: true });

  const anchor = (name: string) => {
    const el = container.querySelector(`[data-tour="${name}"]`);
    expect(el, `no [data-tour="${name}"] on the Settings screen`).not.toBeNull();
    return el as HTMLElement;
  };

  // Appearance: the theme swatches AND the accent row, which is what the
  // stop's words promise.
  const theme = anchor("theme");
  expect(theme.textContent).toContain("Appearance");
  expect(within(theme).getByRole("button", { name: "Theme System" })).toBeInTheDocument();
  expect(within(theme).getAllByRole("button", { name: /^Accent / }).length).toBeGreaterThan(1);

  expect(anchor("settings-backup").textContent).toContain("Backup & transfer");
  const updates = anchor("settings-updates");
  expect(updates.textContent).toContain("Updates");
  expect(within(updates).getByRole("button", { name: "Check for updates" })).toBeInTheDocument();
});

/// Field request: colour the log the way VS Code's Log mode does.
test("the app log colours the level tag and the values in each line", async () => {
  mockIPC((cmd) => {
    if (cmd === "app_logs")
      return [
        { at: "2026-09-11 04:11:18", level: "debug", message: "GET dev.azure.com/acme/_apis/testplan/Plans/107281/suites -> 200 in 184 ms" },
        { at: "2026-09-11 04:11:21", level: "error", message: "Submit failed for 'Login works': Azure DevOps returned HTTP 400" },
      ];
    if (cmd === "app_log_dir") return "C:\logs";
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  fireEvent.click(await screen.findByRole("button", { name: "Logs" }));

  expect(await screen.findByText("[error]")).toHaveClass("text-danger");
  expect(screen.getByText("[debug]")).toHaveClass("text-warning/70");
  expect(screen.getByText("dev.azure.com")).toHaveClass("text-accent");
  expect(screen.getByText("107281")).toHaveClass("text-accent");
  expect(screen.getByText("400")).toHaveClass("text-accent");
  // Nothing is lost between the tokens, and the prose stays the text colour.
  const line = screen.getByText("dev.azure.com").parentElement!;
  expect(line.textContent).toBe("GET dev.azure.com/acme/_apis/testplan/Plans/107281/suites -> 200 in 184 ms");
  expect(line.firstElementChild).toHaveTextContent("GET");
  expect(line.firstElementChild).toHaveClass("text-text");
});

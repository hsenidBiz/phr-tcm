import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import Settings from "./Settings";

afterEach(() => clearMocks());

function renderSettings(qc: QueryClient) {
  return render(
    <QueryClientProvider client={qc}>
      <Settings org="acme" project="Web" />
    </QueryClientProvider>,
  );
}

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

test("the changelog history section lists released versions", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  expect(await screen.findByRole("heading", { name: "Changelog" })).toBeInTheDocument();
  expect(screen.getByText("Version 1.9.0")).toBeInTheDocument();
  expect(screen.getByText("Version 1.7.1")).toBeInTheDocument();
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
  expect(await screen.findByText(/the same notes the post-update popup shows/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Logs" }));
  expect(await screen.findByText("Test Case Manager started")).toBeInTheDocument();
  expect(screen.getByText("Submit failed for 'X'")).toBeInTheDocument();
  // The changelog panel is gone, not merely hidden below.
  expect(
    screen.queryByText(/the same notes the post-update popup shows/),
  ).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Copy log" })).toBeInTheDocument();
});

test("editing default tags saves them for this project as you type", async () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "list_project_tags") return ["smoke", "regression"];
    return undefined;
  });
  renderSettings(qc);

  const field = screen.getByLabelText("Default tags");
  fireEvent.change(field, { target: { value: "smoke" } });
  fireEvent.keyDown(field, { key: "Enter" });

  await waitFor(() =>
    expect(localStorage.getItem("tcm-v2-default-tags:acme/Web")).toBe("smoke"),
  );
  localStorage.clear();
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

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import AiBridge from "./AiBridge";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

// jsdom has no Clipboard API - stub one that returns real Promises so
// `.then()/.catch()` chains in the component resolve/reject like the browser.
let writeText: ReturnType<typeof vi.fn>;
beforeEach(() => {
  writeText = vi.fn(() => Promise.resolve());
  Object.assign(navigator, { clipboard: { writeText } });
});

function renderBridge(qc: QueryClient) {
  return render(
    <QueryClientProvider client={qc}>
      <AiBridge />
    </QueryClientProvider>,
  );
}

test("lists installed AI tools with their registered state", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [
        { id: "claude-code", name: "Claude Code", installed: true, registered_servers: ["tcm-testcases"] },
        { id: "vscode", name: "VS Code", installed: true, registered_servers: [] },
      ];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  expect(await screen.findByText("Claude Code")).toBeInTheDocument();
  expect(screen.getByText("Registered ✓")).toBeInTheDocument();
  expect(screen.getByText("VS Code")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Register" })).toBeInTheDocument();
});

test("Register invokes register_ai_tool with the tool's id", async () => {
  let registeredId: string | undefined;
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [{ id: "vscode", name: "VS Code", installed: true, registered_servers: [] }];
    if (cmd === "register_ai_tool") {
      registeredId = (args as { id: string }).id;
      return null;
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  fireEvent.click(await screen.findByRole("button", { name: "Register" }));
  await waitFor(() => expect(registeredId).toBe("vscode"));
});

test("Unregister invokes unregister_ai_tool for a registered tool", async () => {
  let unregisteredId: string | undefined;
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [{ id: "claude-desktop", name: "Claude Desktop", installed: true, registered_servers: ["tcm-testcases"] }];
    if (cmd === "unregister_ai_tool") {
      unregisteredId = (args as { id: string }).id;
      return null;
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  fireEvent.click(await screen.findByRole("button", { name: "Unregister" }));
  await waitFor(() => expect(unregisteredId).toBe("claude-desktop"));
});

test("Rescan re-runs detection and picks up a newly installed tool", async () => {
  let scans = 0;
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") {
      scans += 1;
      // Second scan sees a tool that wasn't installed at mount.
      return scans === 1
        ? [{ id: "vscode", name: "VS Code", installed: false, registered: false }]
        : [{ id: "vscode", name: "VS Code", installed: true, registered_servers: [] }];
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  // The button reads "Scanning" mid-fetch, so its "Rescan" label is the
  // signal that the first scan finished.
  expect(
    await screen.findByText("No supported AI tools detected on this machine."),
  ).toBeInTheDocument();

  fireEvent.click(await screen.findByRole("button", { name: /Rescan/ }));
  expect(await screen.findByText("VS Code")).toBeInTheDocument();
  expect(scans).toBe(2);
});

test("the how-it-works card names every MCP tool", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  await screen.findByText("How it works");
  for (const name of [
    "begin_test_case_writing",
    "get_writing_guide",
    "get_test_cases",
    "validate_cases",
    "get_tags",
    "optimize_cases",
    "transform_cases",
    "search_pbis",
    "search_wiki",
    "get_wiki_page",
  ]) {
    // Each tool appears twice now - once in the on/off list, once in the
    // explanation below it.
    expect(screen.getAllByText(name).length).toBeGreaterThan(0);
  }
});

test("the copy button writes the registration command to the clipboard", async () => {
  // copyText goes through the Tauri clipboard plugin first - capture that
  // invoke rather than the navigator fallback.
  let copied = "";
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") return [];
    if (String(cmd).startsWith("plugin:clipboard-manager|")) {
      copied = JSON.stringify(args);
      return null;
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  fireEvent.click(await screen.findByText("Other tools"));
  // The exe path is filled in only once the bridge_status query resolves -
  // wait for it so the copied command isn't captured with an empty path.
  // (The path is split across sibling text nodes by JSX interpolation, so
  // match on the element's full textContent rather than a single node.)
  await waitFor(() =>
    expect(
      document.querySelector("code")?.textContent?.includes("v2.exe"),
    ).toBe(true),
  );
  fireEvent.click(screen.getAllByRole("button", { name: "Copy" })[0]);

  await waitFor(() =>
    expect(copied).toContain(
      'claude mcp add --scope user tcm-testcases -- \\"C:\\\\apps\\\\tcm\\\\v2.exe\\" --mcp',
    ),
  );
});

test("shows bridge not running when the status query fails", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") throw "bridge failed to start";
    if (cmd === "detect_ai_tools") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  expect(await screen.findByText("Bridge not running.")).toBeInTheDocument();
});

// ------------------------------------------------- company database server

const DB_TOOLS = [{ id: "vscode", name: "VS Code", installed: true, registered_servers: [] }];

test("the database server cannot be registered until it is configured", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\apps\tcm\v2.exe" };
    if (cmd === "detect_ai_tools") return DB_TOOLS;
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  expect(
    await screen.findByText(/Fill in the executable and connection string/),
  ).toBeInTheDocument();
});

test("configuring the database server persists it and enables registration", async () => {
  let sent: unknown;
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\apps\tcm\v2.exe" };
    if (cmd === "detect_ai_tools") return DB_TOOLS;
    if (cmd === "register_db_server") {
      sent = args;
      return null;
    }
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  fireEvent.change(await screen.findByLabelText("Database server path"), {
    target: { value: "C:/tools/PeoplesHR.DBMCPServer.exe" },
  });
  // The connection string is BUILT from fields - nobody types the whole
  // thing. The stored value is still the single string the server gets.
  fireEvent.change(screen.getByLabelText("Database host"), { target: { value: "db" } });
  fireEvent.change(screen.getByLabelText("Database port"), { target: { value: "1433" } });
  fireEvent.change(screen.getByLabelText("Database name"), { target: { value: "HR" } });
  fireEvent.change(screen.getByLabelText("Database user"), { target: { value: "sa" } });
  fireEvent.change(screen.getByLabelText("Database password"), { target: { value: "p@ss" } });
  fireEvent.change(screen.getByLabelText("Schema filter"), { target: { value: "dbo,hr" } });

  // Kept locally so another editor can be registered without retyping.
  const stored = localStorage.getItem("tcm-v2-db-mcp") as string;
  expect(stored).toContain("PeoplesHR.DBMCPServer.exe");
  expect(JSON.parse(stored).connection_string).toBe(
    "Server=db,1433;Database=HR;User Id=sa;Password=p@ss;TrustServerCertificate=True;",
  );

  // Two Register buttons now: ours and the database server's.
  const buttons = await screen.findAllByRole("button", { name: "Register" });
  fireEvent.click(buttons[buttons.length - 1]);

  await waitFor(() => expect(sent).toBeTruthy());
  const payload = sent as { id: string; config: Record<string, string> };
  expect(payload.id).toBe("vscode");
  expect(payload.config.db_type).toBe("mssql");
  expect(payload.config.schema_filter).toBe("dbo,hr");
  expect(payload.config.connection_string).toContain("Password=p@ss");
});

test("the password is not shown in plain text, in either editing mode", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\apps\tcm\v2.exe" };
    if (cmd === "detect_ai_tools") return DB_TOOLS;
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  const pw = await screen.findByLabelText("Database password");
  expect(pw).toHaveAttribute("type", "password");

  // The raw single-string editor holds the password too, so it is masked
  // just as it was before the builder existed.
  fireEvent.click(screen.getByRole("checkbox", { name: "Edit connection string as text" }));
  expect(screen.getByLabelText("Connection string")).toHaveAttribute("type", "password");
});

/// A string saved before this form existed appears already parsed into the
/// fields - nobody re-enters a working configuration.
test("a stored connection string pre-fills the builder fields", async () => {
  localStorage.setItem(
    "tcm-v2-db-mcp",
    JSON.stringify({
      exe_path: "C:/tools/PeoplesHR.DBMCPServer.exe",
      db_type: "mssql",
      connection_string:
        "Server=phrx-db.internal,1433;Database=PHRX;User Id=reader;Password=old;TrustServerCertificate=True;",
      schema_filter: "",
    }),
  );
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\apps\tcm\v2.exe" };
    if (cmd === "detect_ai_tools") return DB_TOOLS;
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  expect(await screen.findByLabelText("Database host")).toHaveValue("phrx-db.internal");
  expect(screen.getByLabelText("Database port")).toHaveValue("1433");
  expect(screen.getByLabelText("Database name")).toHaveValue("PHRX");
  expect(screen.getByLabelText("Database user")).toHaveValue("reader");

  // Editing ONE field keeps the rest: change the password, the host stays.
  fireEvent.change(screen.getByLabelText("Database password"), { target: { value: "new" } });
  const stored = JSON.parse(localStorage.getItem("tcm-v2-db-mcp") as string);
  expect(stored.connection_string).toBe(
    "Server=phrx-db.internal,1433;Database=PHRX;User Id=reader;Password=new;TrustServerCertificate=True;",
  );
});

/// Picking a shipped environment fills the connection and the schema/type
/// defaults, and persists like any other explicit edit - the dropdown is
/// an act, unlike the silent first-run prefill below.
test("picking a preset fills and persists the connection", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") return [];
    if (cmd === "db_server_presets")
      return [
        { label: "Dev — read only", connection_string: "Server=dev;Database=a;User Id=ro;" },
        { label: "QA — read only", connection_string: "Server=qa;Database=b;User Id=ro;" },
      ];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  fireEvent.click(await screen.findByLabelText("Default connections"));
  fireEvent.click(await screen.findByText("QA — read only"));

  const stored = JSON.parse(localStorage.getItem("tcm-v2-db-mcp") as string);
  expect(stored.connection_string).toBe("Server=qa;Database=b;User Id=ro;");
  expect(stored.db_type).toBe("mssql");
  expect(stored.schema_filter).toBe("PeoplesHR");
});

/// Shipped defaults fill a NEVER-CONFIGURED form only: a fresh machine
/// sees them, a machine with its own saved config keeps it, and nothing
/// registers or persists from the prefill alone.
test("shipped DB defaults prefill only a never-configured form", async () => {
  const DEFAULTS = {
    exe_path: "D:\Phr-Database-McpServer",
    db_type: "mssql",
    connection_string: "Server=sgdev01db02.cloud;Database=phrx;User Id=ro;TrustServerCertificate=True;",
    schema_filter: "PeoplesHR",
  };
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\apps\tcm\v2.exe" };
    if (cmd === "detect_ai_tools") return [];
    if (cmd === "db_server_defaults") return DEFAULTS;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const first = renderBridge(qc);

  // Fresh machine: the form shows the shipped values...
  await waitFor(() =>
    expect(screen.getByLabelText("Database server path")).toHaveValue(DEFAULTS.exe_path),
  );
  // ...without persisting them - prefill is not configuration.
  expect(localStorage.getItem("tcm-v2-db-mcp")).toBeNull();
  first.unmount();

  // A machine with its OWN config never has it overwritten.
  localStorage.setItem(
    "tcm-v2-db-mcp",
    JSON.stringify({
      exe_path: "C:\mine\server.exe",
      db_type: "mssql",
      connection_string: "Server=mine;Database=own;",
      schema_filter: "",
    }),
  );
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  await waitFor(() =>
    expect(screen.getByLabelText("Database server path")).toHaveValue("C:\mine\server.exe"),
  );
});

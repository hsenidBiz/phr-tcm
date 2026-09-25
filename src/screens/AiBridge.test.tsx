import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Toaster } from "../components/ui/toaster";
import AiBridge from "./AiBridge";
import { selectedDbSnapshot, subscribeDbSettings } from "../lib/dbServer";

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

// Every existing test assumes the tab is usable, which now needs a working
// repository. The gating tests below clear it themselves.
beforeEach(() => {
  localStorage.setItem("tcm-v2-working-dir", "D:\\repo");
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
        { id: "claude-code", name: "Claude Code", installed: true, registered_servers: ["tcm-testcases"], scope: "global" },
        { id: "vscode", name: "VS Code", installed: true, registered_servers: [], scope: "global" },
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
      return [{ id: "vscode", name: "VS Code", installed: true, registered_servers: [], scope: "global" }];
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
      return [{ id: "claude-desktop", name: "Claude Desktop", installed: true, registered_servers: ["tcm-testcases"], scope: "global" }];
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
        : [{ id: "vscode", name: "VS Code", installed: true, registered_servers: [], scope: "global" }];
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

test("the AI Tools Breakdown card names every MCP tool", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  const card = (await screen.findByText("AI Tools Breakdown")).closest("section")!;
  // The card explains what you can DECIDE. Every entry has a switch above
  // it, and the tools with no switch are not described here - a paragraph
  // about a control that does not exist is the thing this screen keeps
  // getting rid of.
  // "Find a PBI", not "Find a work item": the query filters on work item
  // type = Product Backlog Item, so it never returns a bug or a task.
  for (const label of ["Test Suites", "Run failures", "Project tags", "Find a PBI", "Project wiki", "Auto Run scripts", "Company database (read)"]) {
    expect(within(card).getByText(label)).toBeInTheDocument();
  }
  for (const label of [
    "Start a writing job",
    "Writing guide",
    "Bulk edits",
    "Build the run sheet",
    "Check a draft",
    "Merge slice files",
    "Auto Run guide",
  ]) {
    expect(within(card).queryByText(label), label).not.toBeInTheDocument();
  }
  // And no identifiers anywhere in it.
  expect(card.textContent).not.toMatch(/[a-z]+_[a-z]/);
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
      'claude mcp add --scope project tcm-testcases -- \\"C:\\\\apps\\\\tcm\\\\v2.exe\\" --mcp',
    ),
  );
});

// The bridge's state itself is the badge beside the tab title
// (BridgeStatusBadge.test.tsx); the tab only has to stay usable without it.
test("the tab still renders when the status query fails", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") throw "bridge failed to start";
    if (cmd === "detect_ai_tools") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  expect(await screen.findByText("Connect your AI tools")).toBeInTheDocument();
  expect(screen.queryByText("Status")).not.toBeInTheDocument();
});

// ------------------------------------------------- company database

const DB_TOOLS = [{ id: "vscode", name: "VS Code", installed: true, registered_servers: [], scope: "global" }];

/// What `db_databases` answers: who signs in and whether a password is
/// saved - never the password or a connection string.
const DATABASES = [
  {
    id: "dev-read", label: "Dev - read only", shipped: true, server: "sgdev01db02.cloud", port: null,
    database: "phrx", user: "sgdev01db02_readonly", trust_cert: true, has_password: true, customised: false,
  },
  {
    id: "dev-login", label: "Dev - dev login", shipped: true, server: "sgdev01db01.cloud", port: null,
    database: "phrx", user: "sgdev01db01_devlogin", trust_cert: true, has_password: true, customised: false,
  },
  {
    id: "qa-read", label: "QA - read only", shipped: true, server: "sgqa01db01.cloud", port: null,
    database: "phrx", user: "sgqa01db01_readonly", trust_cert: true, has_password: true, customised: false,
  },
  {
    id: "own", label: "Your own database", shipped: false, server: "", port: null,
    database: "", user: "", trust_cert: false, has_password: false, customised: false,
  },
];

function dbMocks(extra: (cmd: string, args: unknown) => unknown = () => undefined) {
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") return DB_TOOLS;
    if (cmd === "db_databases") return DATABASES;
    return extra(cmd, args);
  });
}

function dbCard(): HTMLElement {
  return screen.getByText("Company database").closest("section")!;
}

/// The card is which database, its login, and whether it may write. The
/// login itself lives in Rust: nothing on the card can show or take one.
test("the database card is a picker, a login line, Manage credentials and the write switch", async () => {
  localStorage.setItem("tcm-v2-db-selected", "dev-read");
  dbMocks();
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  const picker = await screen.findByRole("combobox", { name: "Database" });
  await waitFor(() => expect(picker).toHaveTextContent("Dev - read only"));
  const card = dbCard();
  expect(within(card).getByText("Signs in as sgdev01db02_readonly")).toBeInTheDocument();
  expect(within(card).getByRole("button", { name: "Manage credentials" })).toBeInTheDocument();
  expect(within(card).getByRole("switch", { name: "Create, update and delete" })).toBeInTheDocument();

  expect(within(card).queryByText("Edit as one string")).not.toBeInTheDocument();
  expect(within(card).queryByText("CONNECTION_STRING")).not.toBeInTheDocument();
  for (const gone of [
    "Database host",
    "Database port",
    "Database name",
    "Database user",
    "Database password",
    "Connection string",
  ]) {
    expect(screen.queryByLabelText(gone)).not.toBeInTheDocument();
  }
  expect(card.querySelector('input[type="password"]')).toBeNull();
});

test("a database with no login saved says so", async () => {
  localStorage.setItem("tcm-v2-db-selected", "own");
  dbMocks();
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  expect(await screen.findByText("No login saved")).toBeInTheDocument();
});

/// Choosing a database is what decides which one the tools run on, so it
/// has to reach the store App pushes from.
test("choosing a database stores its id and tells the bridge subscribers", async () => {
  const told = vi.fn();
  const stop = subscribeDbSettings(told);
  dbMocks();
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  fireEvent.click(await screen.findByRole("combobox", { name: "Database" }));
  fireEvent.click(await screen.findByRole("option", { name: "QA - read only" }));

  expect(localStorage.getItem("tcm-v2-db-selected")).toBe("qa-read");
  expect(selectedDbSnapshot()).toBe("qa-read");
  expect(told).toHaveBeenCalled();
  expect(await screen.findByText("Signs in as sgqa01db01_readonly")).toBeInTheDocument();
  stop();
});

test("Manage credentials opens the login of the chosen database", async () => {
  localStorage.setItem("tcm-v2-db-selected", "dev-login");
  dbMocks();
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  const manage = await screen.findByRole("button", { name: "Manage credentials" });
  await waitFor(() => expect(manage).not.toBeDisabled());
  fireEvent.click(manage);
  expect(
    await screen.findByRole("dialog", { name: "Credentials for Dev - dev login" }),
  ).toBeInTheDocument();
});

test("with no database chosen there is no login to manage", async () => {
  dbMocks();
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  expect(await screen.findByRole("button", { name: "Manage credentials" })).toBeDisabled();
});

test("the database server cannot be registered until it is configured", async () => {
  localStorage.setItem("tcm-v2-ai-show-phrx", "on");
  dbMocks();
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  expect(
    await screen.findByText(/Choose a database and fill in the server path/),
  ).toBeInTheDocument();
});

/// The registration names the database by id; Rust resolves its login.
/// Nothing the webview sends carries a connection string.
test("registering the database server sends the chosen database's id", async () => {
  localStorage.setItem("tcm-v2-ai-show-phrx", "on");
  localStorage.setItem("tcm-v2-db-selected", "qa-read");
  let sent: unknown;
  dbMocks((cmd, args) => {
    if (cmd === "register_db_server") {
      sent = args;
      return null;
    }
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  fireEvent.change(await screen.findByLabelText("Database server path"), {
    target: { value: "C:/tools/PeoplesHR.DBMCPServer.exe" },
  });
  fireEvent.change(screen.getByLabelText("Schema filter"), { target: { value: "dbo,hr" } });

  // Kept locally so another editor can be registered without retyping.
  const stored = localStorage.getItem("tcm-v2-db-mcp") as string;
  expect(stored).toContain("PeoplesHR.DBMCPServer.exe");
  expect(JSON.parse(stored)).not.toHaveProperty("connection_string");

  // Two Register buttons now: ours and the database server's.
  const buttons = await screen.findAllByRole("button", { name: "Register" });
  fireEvent.click(buttons[buttons.length - 1]);

  await waitFor(() => expect(sent).toBeTruthy());
  const payload = sent as { id: string; config: Record<string, string> };
  expect(payload.id).toBe("vscode");
  expect(payload.config).toEqual({
    exe_path: "C:/tools/PeoplesHR.DBMCPServer.exe",
    db_type: "mssql",
    schema_filter: "dbo,hr",
    db_id: "qa-read",
  });
});

/// Shipped defaults fill a NEVER-CONFIGURED form only: a fresh machine
/// sees them, a machine with its own saved config keeps it, and nothing
/// registers or persists from the prefill alone.
test("shipped DB defaults prefill only a never-configured form", async () => {
  const DEFAULTS = {
    exe_path: "D:\\Phr-Database-McpServer",
    db_type: "mssql",
    db_id: "dev-read",
    schema_filter: "PeoplesHR",
  };
  localStorage.setItem("tcm-v2-ai-show-phrx", "on");
  dbMocks((cmd) => {
    if (cmd === "db_server_defaults") return DEFAULTS;
  });
  const first = renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

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
    JSON.stringify({ exe_path: "C:\\mine\\server.exe", db_type: "mssql", schema_filter: "" }),
  );
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  await waitFor(() =>
    expect(screen.getByLabelText("Database server path")).toHaveValue("C:\\mine\\server.exe"),
  );
});

// ------------------------------------------------- working repository gate

/// Several repositories can be saved; the dot picks the CURRENT one, and
/// switching it re-detects against that repository's own configs.
test("saved repositories are listed and switching the current one re-detects", async () => {
  localStorage.removeItem("tcm-v2-working-dir");
  localStorage.setItem(
    "tcm-v2-repositories",
    JSON.stringify([
      { path: "D:\\repo", enabled: true },
      { path: "E:\\other", enabled: true },
    ]),
  );
  localStorage.setItem("tcm-v2-current-repo", "D:\\repo");
  const detectArgs: Array<{ workingDir: string | null }> = [];
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") {
      detectArgs.push(args as { workingDir: string | null });
      return [{ id: "cursor", name: "Cursor", installed: true, registered_servers: [], scope: "project" }];
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);
  expect(await screen.findByText("Cursor")).toBeInTheDocument();
  expect(screen.getByText("D:\\repo")).toBeInTheDocument();
  expect(screen.getByText("E:\\other")).toBeInTheDocument();
  expect(screen.getByRole("radio", { name: "Use D:\\repo" })).toHaveAttribute("aria-checked", "true");
  expect(detectArgs.map((a) => a.workingDir)).toEqual(["D:\\repo"]);

  fireEvent.click(screen.getByRole("radio", { name: "Use E:\\other" }));
  await waitFor(() => expect(detectArgs.map((a) => a.workingDir)).toContain("E:\\other"));
  expect(localStorage.getItem("tcm-v2-current-repo")).toBe("E:\\other");
});

/// The current repository's switch is the off switch for the AI tooling:
/// off closes the gate without forgetting the folder, on reopens it.
test("switching the current repository's AI tools off closes the gate", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [{ id: "cursor", name: "Cursor", installed: true, registered_servers: [], scope: "project" }];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);
  expect(await screen.findByText("Cursor")).toBeInTheDocument();

  fireEvent.click(screen.getByLabelText("AI tools for D:\\repo"));
  await waitFor(() => expect(screen.queryByText("Cursor")).not.toBeInTheDocument());
  expect(screen.queryByRole("button", { name: "Register" })).not.toBeInTheDocument();
  // Still listed - paused, not forgotten.
  expect(screen.getByText("D:\\repo")).toBeInTheDocument();

  fireEvent.click(screen.getByLabelText("AI tools for D:\\repo"));
  expect(await screen.findByText("Cursor")).toBeInTheDocument();
});

test("Remove drops a repository from the list and deselects it", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [{ id: "cursor", name: "Cursor", installed: true, registered_servers: [], scope: "project" }];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);
  expect(await screen.findByText("Cursor")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Remove D:\\repo" }));
  await waitFor(() => expect(screen.queryByText("D:\\repo")).not.toBeInTheDocument());
  expect(screen.queryByText("Cursor")).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-repositories")).toBeNull();
  expect(await screen.findByRole("button", { name: /add repository/i })).toBeInTheDocument();
});

test("without a working repository only the picker is offered", async () => {
  localStorage.removeItem("tcm-v2-working-dir");
  let detected = 0;
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") {
      detected += 1;
      return [{ id: "claude-code", name: "Claude Code", installed: true, registered_servers: [], scope: "global" }];
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  expect(await screen.findByText("Working repositories")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /add repository/i })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Register" })).not.toBeInTheDocument();
  expect(screen.queryByText("Connect your AI tools")).not.toBeInTheDocument();
  expect(detected).toBe(0);
});

test("picking a folder unlocks the tab and detection runs against it", async () => {
  localStorage.removeItem("tcm-v2-working-dir");
  const detectArgs: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "plugin:dialog|open") return "D:\\repo";
    if (cmd === "detect_ai_tools") {
      detectArgs.push(args);
      return [{ id: "claude-code", name: "Claude Code", installed: true, registered_servers: [], scope: "project" }];
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  fireEvent.click(await screen.findByRole("button", { name: /add repository/i }));
  expect(await screen.findByText("Claude Code")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-current-repo")).toBe("D:\\repo");
  expect(detectArgs[0]).toMatchObject({ workingDir: "D:\\repo" });
  expect(screen.getByText("in this repo")).toBeInTheDocument();
});

/// Registering writes this repository's command files, so it has to be told
/// which tools are switched off - otherwise registering hands back the
/// commands for tools the user turned off on this very tab.
/// Machine-wide registration is the pre-per-repo behaviour, kept as an
/// explicit choice for a machine that does not work from a repository:
/// Settings has to allow it, the card then offers it, and choosing it is
/// the one way past the repository gate.
test("with machine-wide allowed, choosing it lifts the gate and registers globally", async () => {
  localStorage.removeItem("tcm-v2-working-dir");
  localStorage.setItem("tcm-v2-ai-global-allowed", "on");
  const detectArgs: unknown[] = [];
  let registered: unknown;
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") {
      detectArgs.push(args);
      return [{ id: "claude-code", name: "Claude Code", installed: true, registered_servers: [], scope: "global" }];
    }
    if (cmd === "register_ai_tool") {
      registered = args;
      return null;
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  // Still gated: the choice defaults to the repository.
  expect(await screen.findByRole("button", { name: "Machine-wide" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Register" })).not.toBeInTheDocument();
  expect(detectArgs).toHaveLength(0);

  fireEvent.click(screen.getByRole("button", { name: "Machine-wide" }));
  expect(await screen.findByText("Claude Code")).toBeInTheDocument();
  expect(detectArgs[0]).toMatchObject({ workingDir: null });
  expect(screen.getByText("global")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Register" }));
  await waitFor(() =>
    expect(registered).toMatchObject({ id: "claude-code", workingDir: null, global: true }),
  );
  expect(localStorage.getItem("tcm-v2-ai-scope")).toBe("global");
});

test("without the Settings switch, the machine-wide choice is not offered", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [{ id: "cursor", name: "Cursor", installed: true, registered_servers: [], scope: "project" }];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  expect(await screen.findByText("Cursor")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Machine-wide" })).not.toBeInTheDocument();
  expect(screen.queryByText("Register in:")).not.toBeInTheDocument();
});

test("Register passes the working repository and the disabled tools along", async () => {
  // A tool that can still be switched: optimize_cases is always on now,
  // so a saved list naming it is ignored on load.
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["get_tags"]));
  let seen: unknown;
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [{ id: "cursor", name: "Cursor", installed: true, registered_servers: [], scope: "project" }];
    if (cmd === "register_ai_tool") {
      seen = args;
      return null;
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  fireEvent.click(await screen.findByRole("button", { name: "Register" }));
  await waitFor(() =>
    expect(seen).toMatchObject({
      id: "cursor",
      workingDir: "D:\\repo",
      disabledTools: ["get_tags"],
    }),
  );
});

// ------------------------------------------- the registration warning

/// The registration worked, but the login it carries is somewhere git can
/// carry it away. That is the one thing on this tab worth reading, so it
/// replaces the success toast rather than sitting in a log.
test("a warning from register_db_server is shown instead of the success toast", async () => {
  const warning =
    "The connection string is in .cursor/mcp.json, which git is tracking in this repository";
  localStorage.setItem("tcm-v2-ai-show-phrx", "on");
  localStorage.setItem("tcm-v2-db-selected", "dev-read");
  dbMocks((cmd) => {
    if (cmd === "register_db_server") return warning;
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  render(<Toaster />);

  fireEvent.change(await screen.findByLabelText("Database server path"), {
    target: { value: "C:/tools/PeoplesHR.DBMCPServer.exe" },
  });

  const buttons = await screen.findAllByRole("button", { name: "Register" });
  fireEvent.click(buttons[buttons.length - 1]);

  expect(await screen.findByText(/git is tracking in this repository/)).toBeInTheDocument();
  expect(screen.queryByText("Database server registered.")).not.toBeInTheDocument();
});

test("no warning means the plain success toast", async () => {
  localStorage.setItem("tcm-v2-ai-show-phrx", "on");
  localStorage.setItem("tcm-v2-db-selected", "dev-read");
  dbMocks((cmd) => {
    if (cmd === "register_db_server") return null;
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  render(<Toaster />);

  fireEvent.change(await screen.findByLabelText("Database server path"), {
    target: { value: "C:/tools/PeoplesHR.DBMCPServer.exe" },
  });

  const buttons = await screen.findAllByRole("button", { name: "Register" });
  fireEvent.click(buttons[buttons.length - 1]);

  expect(await screen.findByText("Database server registered.")).toBeInTheDocument();
});

// ------------------------------------------- leftover global registrations

test("a leftover global registration is surfaced and can be retired", async () => {
  let retired: unknown;
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [
        {
          id: "claude-code", name: "Claude Code", installed: true,
          registered_servers: ["tcm-testcases"], scope: "project",
          global_registered_servers: ["tcm-testcases"],
        },
      ];
    if (cmd === "retire_global_registrations") {
      retired = args;
      return null;
    }
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  expect(await screen.findByText("also registered globally")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Retire global copies" }));
  await waitFor(() => expect(retired).toMatchObject({ id: "claude-code" }));
});

test("a tool with nothing left globally is not offered the retire button", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [
        {
          id: "claude-code", name: "Claude Code", installed: true,
          registered_servers: ["tcm-testcases"], scope: "project",
          global_registered_servers: [],
        },
      ];
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  expect(await screen.findByText("Claude Code")).toBeInTheDocument();
  expect(screen.queryByText("also registered globally")).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Retire global copies" }),
  ).not.toBeInTheDocument();
});

/// The database list carries the same scope label as the list above it:
/// which config a login is about to go into is exactly what a person needs
/// to know before clicking Register.
test("the database server list labels each row's scope too", async () => {
  localStorage.setItem("tcm-v2-ai-show-phrx", "on");
  localStorage.setItem("tcm-v2-db-selected", "dev-read");
  localStorage.setItem(
    "tcm-v2-db-mcp",
    JSON.stringify({ exe_path: "C:/tools/PeoplesHR.DBMCPServer.exe", db_type: "mssql", schema_filter: "" }),
  );
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [{ id: "cursor", name: "Cursor", installed: true, registered_servers: [], scope: "project" }];
    if (cmd === "db_databases") return DATABASES;
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  // Once in the tools list, once in the database list.
  await waitFor(() => expect(screen.getAllByText("in this repo")).toHaveLength(2));
});

test("a tool with no project config is labelled global", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [{ id: "windsurf", name: "Windsurf", installed: true, registered_servers: ["tcm-testcases"], scope: "global" }];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  expect(await screen.findByText("Windsurf")).toBeInTheDocument();
  expect(screen.getByText("global")).toBeInTheDocument();
});

/// Choosing a different database must reach the FILE, not just the card:
/// every tool the database server is already registered in gets
/// re-registered with the new id, and the toast tells the user the one
/// thing left to do - restart the coding session that read the old file at
/// startup. Tools without the server registered are left alone.
test("choosing a database re-registers the DB server where it is registered, then says to restart", async () => {
  // The PHR X option is on: this is the case where syncing a leftover
  // registration's config is exactly what should happen.
  localStorage.setItem("tcm-v2-ai-show-phrx", "on");
  const registered: Array<{ id: string; dbId: string }> = [];
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [
        { id: "vscode", name: "VS Code", installed: true, registered_servers: ["phr-db-mcp"], scope: "global" },
        { id: "cursor", name: "Cursor", installed: true, registered_servers: [], scope: "global" },
      ];
    if (cmd === "db_databases") return DATABASES;
    if (cmd === "register_db_server") {
      const a = args as { id: string; config: { db_id: string } };
      registered.push({ id: a.id, dbId: a.config.db_id });
      return null;
    }
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  render(<Toaster />);

  // The tool list has to be in before the pick, or there is nothing to sync.
  await screen.findAllByText("VS Code");
  fireEvent.click(await screen.findByRole("combobox", { name: "Database" }));
  fireEvent.click(await screen.findByRole("option", { name: "QA - read only" }));

  await waitFor(() => expect(registered).toHaveLength(1));
  expect(registered[0]).toEqual({ id: "vscode", dbId: "qa-read" });
  expect(await screen.findByText(/coding session may need to be restarted/)).toBeInTheDocument();
});

/// With the PHR X option off, choosing a database must never re-register
/// the separate server, even where a leftover registration still exists
/// and a stored config still carries its exe_path - that would silently
/// refresh a login copy the leftover notice tells people to remove.
test("with the PHR X option off, choosing a database does not sync a leftover PHR X registration", async () => {
  localStorage.setItem(
    "tcm-v2-db-mcp",
    JSON.stringify({ exe_path: "C:/tools/PeoplesHR.DBMCPServer.exe", db_type: "mssql", schema_filter: "PeoplesHR" }),
  );
  let registerCalled = false;
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [
        { id: "vscode", name: "VS Code", installed: true, registered_servers: ["phr-db-mcp"], scope: "global" },
      ];
    if (cmd === "db_databases") return DATABASES;
    if (cmd === "register_db_server") {
      registerCalled = true;
      return null;
    }
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  await screen.findByText(/still registered with the tools below/);
  fireEvent.click(await screen.findByRole("combobox", { name: "Database" }));
  fireEvent.click(await screen.findByRole("option", { name: "QA - read only" }));

  await waitFor(() => expect(localStorage.getItem("tcm-v2-db-selected")).toBe("qa-read"));
  expect(registerCalled).toBe(false);
});

test("the tool list offers only the switchable tools, by their human names", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") return [];
    if (cmd === "db_server_defaults") return null;
    return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);
  await screen.findByText("Tools an assistant may use");
  const toolSection = screen.getByText("Tools an assistant may use").closest("section")!;
  // Not one identifier anywhere in this card: the screen names tools the
  // way a person would say them.
  expect(toolSection.textContent).not.toMatch(/[a-z]+_[a-z]/);
  // The always-on tools carry no switch, so they are not offered here. A
  // row that cannot be changed was a control that did nothing.
  expect(within(toolSection).queryByText("Start a writing job")).not.toBeInTheDocument();
  expect(within(toolSection).queryByText("Bulk edits")).not.toBeInTheDocument();
  expect(within(toolSection).queryByText("Build the run sheet")).not.toBeInTheDocument();
  expect(within(toolSection).queryByText("Check a draft")).not.toBeInTheDocument();
  expect(within(toolSection).queryByText("Merge slice files")).not.toBeInTheDocument();
  expect(screen.queryByText("always on")).not.toBeInTheDocument();
  // Six rows for fourteen tools, in this development build: the wiki
  // search and its page reader share one switch, so do the suite search
  // and its case reader, and so do the seven Auto Run tools.
  expect(screen.getByLabelText("Project tags")).toBeInTheDocument();
  expect(screen.getByLabelText("Project wiki")).toBeInTheDocument();
  expect(screen.getByLabelText("Test Suites")).toBeInTheDocument();
  expect(screen.getByLabelText("Auto Run scripts")).toBeInTheDocument();
  expect(within(toolSection).getByText("7 of 7 on")).toBeInTheDocument();
});

test("switching the Auto Run scripts row off sends every tool name in the disabled list", async () => {
  let seen: unknown;
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [{ id: "cursor", name: "Cursor", installed: true, registered_servers: [], scope: "project" }];
    if (cmd === "db_server_defaults") return null;
    if (cmd === "register_ai_tool") {
      seen = args;
      return null;
    }
    return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  fireEvent.click(await screen.findByLabelText("Auto Run scripts"));

  fireEvent.click(await screen.findByRole("button", { name: "Register" }));
  await waitFor(() =>
    expect(seen).toMatchObject({
      id: "cursor",
      workingDir: "D:\\repo",
    }),
  );
  const disabledTools = (seen as { disabledTools: string[] }).disabledTools;
  expect([...disabledTools].sort()).toEqual([
    "get_autorun_failures",
    "get_autorun_guide",
    "get_autorun_page",
    "probe_autorun_locator",
    "record_autorun_quirk",
    "save_autorun_script",
    "try_autorun_action",
  ]);
});

// --------------------------------------------- the database tools' switches

/// The card is about the database the app's OWN tools use now, and it is
/// always there - registering the separate PHR X server is opt-in from
/// Settings, off by default.
test("the database is always shown; the PHR X option is not, by default", async () => {
  dbMocks(); // none has phr-db-mcp registered
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  expect(await screen.findByText("Company database")).toBeInTheDocument();
  expect(await screen.findByRole("combobox", { name: "Database" })).toBeInTheDocument();
  expect(screen.queryByLabelText("Database server path")).not.toBeInTheDocument();
  expect(screen.queryByText(/no longer needed for lookups/)).not.toBeInTheDocument();
  // Nothing is registered, so there is no leftover to remove either.
  expect(screen.queryByText(/still registered with the tools below/)).not.toBeInTheDocument();
  // The Forget paragraph's clause about unregistering only makes sense
  // when there is a PHR X part to point at.
  expect(screen.queryByText(/Unregister above to remove them/)).not.toBeInTheDocument();
});

test("switched on in Settings, the PHR X option appears as before", async () => {
  localStorage.setItem("tcm-v2-ai-show-phrx", "on");
  dbMocks();
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  expect(await screen.findByLabelText("Database server path")).toBeInTheDocument();
  expect(screen.getByText(/no longer needed for lookups/)).toBeInTheDocument();
  localStorage.clear();
});

test("with the option off, a tool that still has PHR X registered can unregister it", async () => {
  const calls: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [
        { id: "claude-code", name: "Claude Code", installed: true, registered_servers: ["tcm-testcases", "phr-db-mcp"], scope: "project" },
        { id: "vscode", name: "VS Code", installed: true, registered_servers: ["tcm-testcases"], scope: "project" },
      ];
    if (cmd === "db_databases") return DATABASES;
    if (cmd === "unregister_db_server") { calls.push((args as { id: string }).id); return null; }
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  await screen.findByText(/still registered with the tools below/);
  // No executable path configured, and still the row is there to remove it.
  expect(screen.queryByLabelText("Database server path")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Register$/ })).not.toBeInTheDocument();
  // The Forget paragraph now has something to point at.
  expect(screen.getByText(/Unregister above to remove them/)).toBeInTheDocument();
  // The accessible name says which tool, so it is unambiguous even next to
  // the "Connect your AI tools" list above, where both tools also show an
  // Unregister button for the unrelated tcm-testcases server.
  fireEvent.click(
    screen.getByRole("button", { name: "Unregister the PHR X server from Claude Code" }),
  );
  await waitFor(() => expect(calls).toEqual(["claude-code"]));
});

/// Off by default, and unmovable on a database the backend would refuse
/// the write on anyway.
test("creating, updating and deleting is off, and disabled on a read-only database", async () => {
  localStorage.setItem("tcm-v2-db-selected", "dev-read");
  dbMocks();
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  await screen.findByText("Signs in as sgdev01db02_readonly");
  const writes = screen.getByRole("switch", { name: "Create, update and delete" });
  expect(writes).toHaveAttribute("aria-checked", "false");
  expect(writes).toBeDisabled();
  // And the reason it cannot be moved is on screen, not implied.
  expect(screen.getByText(/Only on a dev login database/)).toBeInTheDocument();

  fireEvent.click(writes);
  expect(localStorage.getItem("tcm-v2-db-writes")).toBeNull();
});

/// On the dev login it moves, and the stored flag is what App pushes to
/// the bridge beside the database id.
test("on the dev login the write switch turns on and is stored", async () => {
  localStorage.setItem("tcm-v2-db-selected", "dev-login");
  dbMocks();
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  await screen.findByText("Signs in as sgdev01db01_devlogin");
  const writes = screen.getByRole("switch", { name: "Create, update and delete" });
  expect(writes).not.toBeDisabled();
  expect(writes).toHaveAttribute("aria-checked", "false");

  fireEvent.click(writes);
  expect(localStorage.getItem("tcm-v2-db-writes")).toBe("1");
  expect(
    await screen.findByRole("switch", { name: "Create, update and delete" }),
  ).toHaveAttribute("aria-checked", "true");
});

/// Forgetting takes every saved login, the choice and permission to write
/// with it. Leaving writes standing would hand the next database a
/// decision nobody made about it.
test("Forget them wipes the saved logins, the choice and the write switch", async () => {
  localStorage.setItem("tcm-v2-db-writes", "1");
  localStorage.setItem("tcm-v2-db-selected", "dev-login");
  localStorage.setItem(
    "tcm-v2-db-mcp",
    JSON.stringify({ exe_path: "C:/x.exe", db_type: "mssql", schema_filter: "" }),
  );
  let forgot = 0;
  dbMocks((cmd) => {
    if (cmd === "forget_db_credentials") {
      forgot += 1;
      return null;
    }
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  render(<Toaster />);

  fireEvent.click(await screen.findByText("Forget them"));
  await waitFor(() => expect(forgot).toBe(1));
  expect(await screen.findByText("Database settings forgotten.")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-db-writes")).toBeNull();
  expect(localStorage.getItem("tcm-v2-db-mcp")).toBeNull();
  expect(localStorage.getItem("tcm-v2-db-selected")).toBeNull();
});

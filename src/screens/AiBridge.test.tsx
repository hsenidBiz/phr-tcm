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
  for (const label of ["Test Suites", "Run failures", "Project tags", "Find a PBI", "Project wiki", "Auto Run scripts", "API templates", "Company database (read)"]) {
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

// Fix round 1 (Task 3), corrected in round 2: the AI Tools tab's own
// display gate, autoRunToolsShown() (autoRunToolsOffered() && !capture),
// now hides every render site - the switch row, the breakdown-card entry
// and the numbered-list bullet - in capture mode. autoRunToolsOffered()
// itself stays capture-blind, since it also decides what gets saved and
// registered (see mcpTools.test.ts). This is a documented, captured
// screen; off, the tests above and below pin that this build's ordinary
// behaviour is unchanged.
//
// The API templates card is gated by the very same function (Task 7), so
// capture mode hiding it here also stands in for a locked release build -
// autoRunToolsOffered() (DEV_BUILD || extras unlocked) is what
// autoRunToolsShown() is built on, and that function's own DEV_BUILD=false
// behaviour is covered in mcpTools.test.ts.
test("capture mode hides every mention of the Auto Run tools, and the API templates card, on the AI Tools tab", async () => {
  localStorage.setItem("tcm-v2-dev-capture", "on");
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") return [];
    return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  await screen.findByText("Tools an assistant may use");
  expect(screen.queryByLabelText("Auto Run scripts")).not.toBeInTheDocument();
  expect(screen.queryByText("Auto Run scripts")).not.toBeInTheDocument();
  expect(screen.queryByText(/Auto Run stays something you drive by hand/)).not.toBeInTheDocument();
  expect(screen.queryByText("API templates")).not.toBeInTheDocument();
  expect(
    screen.queryByRole("switch", { name: "API templates (create, edit and delete)" }),
  ).not.toBeInTheDocument();
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
  // The two Copy buttons are named for what they copy.
  expect(screen.getByRole("button", { name: "Copy config" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Copy command" }));

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
    database: "hrmmain", user: "sgdev01db02_readonly", trust_cert: true, has_password: true, customised: false,
  },
  {
    id: "dev-login", label: "Dev - dev login", shipped: true, server: "sgdev01db01.cloud", port: null,
    database: "hrmmain", user: "sgdev01db01_devlogin", trust_cert: true, has_password: true, customised: false,
  },
  {
    id: "qa-read", label: "QA - read only", shipped: true, server: "sgqa01db01.cloud", port: null,
    database: "hrmmain", user: "sgqa01db01_readonly", trust_cert: true, has_password: true, customised: false,
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

test("the tool list offers only the switchable tools, by their human names", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") return [];
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
  // Eight rows in this development build: the wiki search and its page
  // reader share one switch, so do the suite search and its case reader,
  // the two database tools, the seven Auto Run tools and the six API
  // template tools.
  expect(screen.getByLabelText("Project tags")).toBeInTheDocument();
  expect(screen.getByLabelText("Project wiki")).toBeInTheDocument();
  expect(screen.getByLabelText("Test Suites")).toBeInTheDocument();
  expect(screen.getByLabelText("Auto Run scripts")).toBeInTheDocument();
  expect(screen.getByLabelText("API templates")).toBeInTheDocument();
  expect(within(toolSection).getByText("8 of 8 on")).toBeInTheDocument();
});

test("the API templates breakdown says the assistant maps a module's stages and checks the order before a run", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\apps\tcm\v2.exe" };
    if (cmd === "detect_ai_tools") return [];
    return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);
  await screen.findByText("Tools an assistant may use");
  const entry = [...document.querySelectorAll("li")].find((li) => li.textContent?.startsWith("API templates:"))!;
  expect(entry, "the API templates breakdown entry").toBeTruthy();
  expect(entry.textContent).toMatch(/flow/i);
  expect(entry.textContent).toMatch(/stage/i);
  // The stage checks read the database, so they need reading switched on.
  expect(entry.textContent).toMatch(/need Company database \(read\) switched on/);
  expect(entry.textContent).not.toMatch(/[a-z]+_[a-z]/);
});

test("switching the Auto Run scripts row off sends every tool name in the disabled list", async () => {
  let seen: unknown;
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [{ id: "cursor", name: "Cursor", installed: true, registered_servers: [], scope: "project" }];
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

/// The card is about the database the app's OWN tools use, and nothing
/// else: no second server to point at, and no switch in Settings to bring
/// one back.
test("the database card offers no separate database server", async () => {
  dbMocks();
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  expect(await screen.findByRole("combobox", { name: "Database" })).toBeInTheDocument();
  const card = dbCard();
  for (const gone of ["Database server path", "Database type", "Schema filter"]) {
    expect(screen.queryByLabelText(gone)).not.toBeInTheDocument();
  }
  expect(within(card).queryByRole("button", { name: /register/i })).not.toBeInTheDocument();
  expect(card.textContent).not.toMatch(/MCP server|DBMCPServer|Unregister/);
  expect(within(card).getByText(/Logins are kept in Windows Credential Manager/)).toHaveTextContent(
    "Logins are kept in Windows Credential Manager and the other settings on this machine. Forget them.",
  );
});

// ------------------------------------- the old database server's leftovers

type Removal = { id: string; workingDir: string | null; global: boolean };

/// Every command and log line the quiet cleanup produces, from one mock.
function legacyMocks(tools: unknown[], removal: (args: Removal) => unknown = () => null) {
  const removed: Removal[] = [];
  const logged: string[] = [];
  let scans = 0;
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") {
      scans += 1;
      return tools;
    }
    if (cmd === "db_databases") return DATABASES;
    if (cmd === "log_ui") {
      logged.push((args as { message: string }).message);
      return null;
    }
    if (cmd === "remove_legacy_db_server") {
      const a = args as Removal;
      removed.push({ id: a.id, workingDir: a.workingDir, global: a.global });
      return removal(a);
    }
  });
  return { removed, logged, scans: () => scans };
}

/// An earlier version could register a separate database server with a
/// tool. The tab removes that entry by itself - only from the tools that
/// still list it, through the repository config the scan read - logs it,
/// and says nothing about it anywhere on screen.
test("a tool still carrying the old database server has it removed quietly", async () => {
  const { removed, logged } = legacyMocks([
    { id: "claude-code", name: "Claude Code", installed: true, registered_servers: ["tcm-testcases", "phr-db-mcp"], scope: "project" },
    { id: "vscode", name: "VS Code", installed: true, registered_servers: ["tcm-testcases"], scope: "project" },
    { id: "cursor", name: "Cursor", installed: true, registered_servers: [], scope: "project" },
  ]);
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  render(<Toaster />);

  await waitFor(() => expect(removed).toEqual([{ id: "claude-code", workingDir: "D:\\repo", global: false }]));
  await waitFor(() =>
    expect(logged).toContain("AI tools: removed the old database server from claude-code (project config)"),
  );
  // Our own server is still shown as registered, on both tools that have it.
  expect(screen.getAllByText("Registered ✓")).toHaveLength(2);
  // Nothing on screen names the old server - no notice, no button, no toast,
  // not even an accessible name or a tooltip.
  expect(document.body.textContent).not.toMatch(/phr|old database server|still registered/i);
  for (const el of document.querySelectorAll("[aria-label], [title]")) {
    expect(`${el.getAttribute("aria-label") ?? ""} ${el.getAttribute("title") ?? ""}`).not.toMatch(/phr/i);
  }
  expect(removed).toHaveLength(1);
});

/// A failed removal is logged and left for the next scan: never retried in
/// a loop, and never shown. A new scan (Rescan) tries once more - once.
test("a failed removal is logged once per scan and never loops", async () => {
  const { removed, logged, scans } = legacyMocks(
    [{ id: "vscode", name: "VS Code", installed: true, registered_servers: ["phr-db-mcp"], scope: "project" }],
    () => {
      throw "failed to read D:\\repo\\.vscode\\mcp.json: access denied";
    },
  );
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  render(<Toaster />);

  await waitFor(() => expect(logged.some((m) => m.startsWith("AI tools: could not remove"))).toBe(true));
  // Give a loop every chance to show itself.
  await new Promise((r) => setTimeout(r, 300));
  expect(removed).toHaveLength(1);
  expect(scans()).toBe(1);
  expect(document.body.textContent).not.toMatch(/phr|could not remove|access denied/i);

  fireEvent.click(screen.getByRole("button", { name: /Rescan/ }));
  await waitFor(() => expect(scans()).toBe(2));
  await waitFor(() => expect(removed).toHaveLength(2));
  await new Promise((r) => setTimeout(r, 300));
  expect(removed).toHaveLength(2);
});

/// The cleanup happens once per tool and config. An entry that is back
/// after a removal that worked was added by hand - someone still runs that
/// server - and is theirs: later scans leave it alone.
test("an entry added back by hand after the cleanup is left alone", async () => {
  const { removed, scans } = legacyMocks([
    { id: "vscode", name: "VS Code", installed: true, registered_servers: ["phr-db-mcp"], scope: "project" },
  ]);
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  await waitFor(() => expect(removed).toHaveLength(1));
  // The mock's scan still lists it, as a hand-added entry would.
  fireEvent.click(screen.getByRole("button", { name: /Rescan/ }));
  await waitFor(() => expect(scans()).toBe(2));
  await new Promise((r) => setTimeout(r, 300));
  expect(removed).toHaveLength(1);
});

/// The same leftover in the machine-wide config while the row reads the
/// repository's is removed from there - and it alone does not make the row
/// say "also registered globally", which is about our own server.
test("the old server's machine-wide copy is removed from the global config, unannounced", async () => {
  const { removed } = legacyMocks([
    {
      id: "cursor", name: "Cursor", installed: true,
      registered_servers: ["tcm-testcases"], scope: "project",
      global_registered_servers: ["phr-db-mcp"],
    },
  ]);
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  await waitFor(() => expect(removed).toEqual([{ id: "cursor", workingDir: null, global: true }]));
  expect(screen.queryByText("also registered globally")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Retire global copies" })).not.toBeInTheDocument();
});

/// A tool that is not installed is not touched: the cleanup follows the
/// installed tools the tab lists.
test("nothing is removed from a tool that is not installed", async () => {
  const { removed, scans } = legacyMocks([
    { id: "windsurf", name: "Windsurf", installed: false, registered_servers: ["phr-db-mcp"], scope: "global" },
  ]);
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  await screen.findByText("No supported AI tools detected on this machine.");
  await waitFor(() => expect(scans()).toBe(1));
  await new Promise((r) => setTimeout(r, 100));
  expect(removed).toEqual([]);
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

/// The API templates switch is a separate decision from whether the four
/// tools are reachable at all (the "API templates" row further up the
/// tool list): off by default, and the stored flag is what App pushes to
/// the bridge as apiWrites, the same shape as the database write switch.
test("the API templates switch is off, and stored when turned on", async () => {
  dbMocks();
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  const writes = await screen.findByRole("switch", { name: "API templates (create, edit and delete)" });
  expect(writes).toHaveAttribute("aria-checked", "false");
  expect(localStorage.getItem("tcm-v2-api-writes")).toBeNull();

  fireEvent.click(writes);
  expect(localStorage.getItem("tcm-v2-api-writes")).toBe("1");
  expect(
    await screen.findByRole("switch", { name: "API templates (create, edit and delete)" }),
  ).toHaveAttribute("aria-checked", "true");
});

/// "Run changes without asking" means something only while writes can
/// happen, and turning it on reports what each registered tool needs:
/// the ones the app set say nothing, the rest say what to do.
test("running changes without asking needs writes on, and says what each tool needs", async () => {
  localStorage.setItem("tcm-v2-db-selected", "dev-login");
  const asked: unknown[] = [];
  let settings = { close_to_tray: true, close_notice_shown: false, beta_updates: false, start_minimized: true, db_auto_approve: false };
  dbMocks((cmd, args) => {
    if (cmd === "get_app_settings") return settings;
    if (cmd === "set_db_auto_approve") {
      asked.push(args);
      settings = { ...settings, db_auto_approve: (args as { on: boolean }).on };
      return [
        { tool: "Claude Code", applied: true, note: "" },
        { tool: "VS Code", applied: false, note: "VS Code keeps this in its own window: when it asks about db_query, choose to always allow it (or use Chat: Manage Tool Approval)" },
      ];
    }
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  await screen.findByText("Signs in as sgdev01db01_devlogin");

  const noAsk = screen.getByRole("switch", { name: "Run database changes without asking" });
  expect(noAsk).toBeDisabled();
  expect(noAsk).toHaveAttribute("aria-checked", "false");

  fireEvent.click(screen.getByRole("switch", { name: "Create, update and delete" }));
  await waitFor(() =>
    expect(screen.getByRole("switch", { name: "Run database changes without asking" })).not.toBeDisabled(),
  );
  fireEvent.click(screen.getByRole("switch", { name: "Run database changes without asking" }));

  await waitFor(() => expect(asked).toHaveLength(1));
  expect((asked[0] as { on: boolean }).on).toBe(true);
  expect(await screen.findByText(/VS Code keeps this in its own window/)).toBeInTheDocument();
  expect(screen.queryByText(/Claude Code keeps/)).not.toBeInTheDocument();
  await waitFor(() =>
    expect(screen.getByRole("switch", { name: "Run database changes without asking" })).toHaveAttribute(
      "aria-checked",
      "true",
    ),
  );
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

/// The risk-tiered writing guide is a trial: off, the assistant writes cases
/// with the standard guide. The switch is remembered on this machine, and
/// App pushes it to the bridge with the other switches.
test("the test design card switches the risk-tiered writing guide on and off", async () => {
  dbMocks();
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

  const card = (await screen.findByRole("heading", { name: "Test design rules" })).closest("section")!;
  const trial = within(card).getByRole("switch", { name: "Risk-tiered test design (trial)" });
  expect(trial).toHaveAttribute("aria-checked", "false");
  expect(card).toHaveTextContent("Off: the assistant writes cases with the standard guide.");
  expect(card).toHaveTextContent("lists the scenarios for your approval before writing");

  fireEvent.click(trial);
  expect(trial).toHaveAttribute("aria-checked", "true");
  expect(localStorage.getItem("tcm-v2-risk-tiered-guide")).toBe("1");

  fireEvent.click(trial);
  expect(trial).toHaveAttribute("aria-checked", "false");
  expect(localStorage.getItem("tcm-v2-risk-tiered-guide")).toBeNull();
});

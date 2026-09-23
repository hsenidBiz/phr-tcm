# Company Database: Own Connection, PHR X Off by Default Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Owner request 2026-09-23, after confirming the app's own sqlcmd database tools work: the separate PHR X DB server option is off by default and people use the app's own database tools; people whose database is not one of the shipped presets can clearly enter their own server, database, user and password.

**Architecture:** Frontend only. The AI Bridge tab's "Company database" card has two parts: the connection (used by the app's own `db_lookup`/`db_query` tools) and, below it, registration of the separate PHR X server (`phr-db-mcp`). Today one Settings switch, on by default, hides or shows the whole card. After this plan the connection part is always shown, and only the PHR X part sits behind a Settings switch that is off by default (a new storage key, so existing installs start off too). A PHR X registration left in an AI tool's config stays removable: with the switch off, any tool that still has `phr-db-mcp` registered is listed with Unregister. The connection picker gains a "Your own database" choice.

**Tech Stack:** React 19 + TypeScript, vitest + Testing Library, `mockIPC`.

## Global Constraints

- One build or test command at a time (shared machine). Frontend tests: `npx vitest run <paths> --exclude "**/.claude/**"`; typecheck `npx tsc --noEmit`. No Rust change in this plan, so `src/bindings.ts` must not change (a line-ending-only diff is reverted with `git checkout -- src/bindings.ts`).
- No new npm packages. No em dashes in new user-facing text. Colours via tokens; icons from `src/lib/actionIcons.ts`; dropdowns are the themed `Combobox`/`Select`; `src/ui-consistency.test.ts` must not be weakened.
- The write rule does not change: writes need the create/update/delete switch AND a connection whose user id ends `_devlogin`. A hand-entered connection is read only unless its user is a dev login.
- The PHR X registration code (Rust `register_db_server`/`unregister_db_server`, `DB_SERVER = "phr-db-mcp"`) stays; this plan changes when the UI offers it, not whether it can run.
- Commits via Bash heredoc with the model's own `Co-Authored-By` trailer. The changelog is written at release time.

**Out of scope:** removing PHR X support from the code; automatically unregistering `phr-db-mcp` from anyone's config files (the app only edits a tool's config when the person clicks); excluding the connection from Backup & transfer (a known, separate question); letting a non-dev-login connection write.

## Tasks

1. PHR X is an option you switch on, and a leftover registration can still be removed
2. "Your own database" in the connection picker

---

### Task 1: PHR X is an option you switch on, and a leftover registration can still be removed

**Files:**
- Modify: `src/lib/aiScope.ts:70-99` (the show-db setting)
- Modify: `src/screens/Settings.tsx:17`, `:74`, `:335-345` (the switch)
- Modify: `src/screens/AiBridge.tsx:96-98` (subscription), `:614` (card gate), `:796-918` (the PHR X part), the "Forget them" paragraph just before the card's closing `</section>`
- Modify: `README.md:137-164` (AI Bridge section)
- Test: `src/lib/aiScope.test.ts`, `src/screens/Settings.test.tsx:265-277`, `src/screens/AiBridge.test.tsx:806-841`

**Interfaces (produces):**
- `src/lib/aiScope.ts` replaces `loadShowDb`/`saveShowDb`/`showDbSnapshot`/`subscribeShowDb` with:
  ```ts
  const SHOW_PHRX_KEY = "tcm-v2-ai-show-phrx";
  /** Whether the AI Bridge tab offers registering the separate PHR X DB server.
   * Off by default: the app's own database tools replaced it. Only the ON
   * choice is stored. A new key rather than the old show-db one, whose
   * default was on and hid the whole database card, connection included. */
  export function loadShowPhrx(): boolean   // localStorage.getItem(KEY) === "on"; false on any storage error
  export function saveShowPhrx(on: boolean): void   // on -> setItem(KEY, "on"); off -> removeItem(KEY); then notify()
  export function showPhrxSnapshot(): boolean
  export const subscribeShowPhrx = subscribeAiScope;
  ```
  The old key `tcm-v2-ai-show-db` is no longer read by anything. Leave stored values alone (no migration: off-by-default is the point).
- Settings switch: `ariaLabel` and visible text `Offer the PHR X database server on the AI Bridge tab`, bound to `loadShowPhrx`/`saveShowPhrx`, unchecked on a fresh profile.
- `AiBridge.tsx`:
  - The `Company database` section (`data-tour="ai-db"`) renders always (drop the `(showDb || tourRunning) &&` gate; `tourRunning` stays only if still used elsewhere).
  - `const showPhrx = useSyncExternalStore(subscribeShowPhrx, showPhrxSnapshot);`
  - `const phrxLeftover = installed.filter((t) => (t.registered_servers ?? []).includes(DB_SERVER));`
  - When `showPhrx` is true: the PHR X part renders exactly as today.
  - When `showPhrx` is false and `phrxLeftover.length > 0`: render only a notice and one row per leftover tool with its Unregister button (the existing `unregisterDb` mutation, `IconUnregister`, the same "Removing" label). No exe path, DB_TYPE, schema filter or Register controls. This must not depend on `dbReady` (today the rows are hidden unless the executable path is filled in, which would strand a leftover registration on a fresh profile). Notice text, verbatim: `The PHR X database server is still registered with the tools below. This app's own database tools replace it, and its registration keeps the connection string, password included, in that tool's settings file. Unregister it to remove that copy.`
  - When `showPhrx` is false and nothing is registered: nothing of the PHR X part renders.
  - The `Forget them` paragraph: the trailing clause about unregistering (currently `— this clears the form only; unregister above to remove them from a tool.`) is shown only when the PHR X part is visible (either mode), and its em dash becomes a full stop: `This clears the form only. Unregister above to remove them from a tool.`
- `README.md`: the bullet `An optional company database server registers alongside the bridge...` is rewritten so the app's own tools come first and PHR X is described as an older option, off unless switched on in Settings. The paragraph ending `...the connection's password is one of its own command-line arguments, so it is readable from this machine's process list for that moment...` is corrected: since 1.25.18 the password reaches `sqlcmd` through its `SQLCMDPASSWORD` environment variable rather than its command line, so it no longer appears in the process list. Do not claim nothing else can read it. Keep the point that a password typed into this form is stored on this machine in the app's settings, in plain text.

- [ ] **Step 1: Write the failing tests**

`src/lib/aiScope.test.ts` (add; import the new functions):
```ts
test("the PHR X option is off by default and only ON is stored", () => {
  localStorage.clear();
  expect(loadShowPhrx()).toBe(false);
  saveShowPhrx(true);
  expect(localStorage.getItem("tcm-v2-ai-show-phrx")).toBe("on");
  expect(loadShowPhrx()).toBe(true);
  saveShowPhrx(false);
  expect(localStorage.getItem("tcm-v2-ai-show-phrx")).toBeNull();
  expect(loadShowPhrx()).toBe(false);
});

test("the old show-db key no longer switches anything on", () => {
  localStorage.clear();
  localStorage.setItem("tcm-v2-ai-show-db", "on");
  expect(loadShowPhrx()).toBe(false);
});
```
If `aiScope.test.ts` has tests for `loadShowDb`/`saveShowDb`, replace them with these (they test a setting that no longer exists).

`src/screens/Settings.test.tsx` - replace `the PHR-X card switch persists its choice, on by default` (line ~265) with:
```ts
test("the PHR X option switch is off by default and persists ON only", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  const sw = await screen.findByLabelText("Offer the PHR X database server on the AI Bridge tab");
  expect(sw).not.toBeChecked();
  fireEvent.click(sw);
  expect(localStorage.getItem("tcm-v2-ai-show-phrx")).toBe("on");
  fireEvent.click(sw);
  expect(localStorage.getItem("tcm-v2-ai-show-phrx")).toBeNull();
  localStorage.clear();
});
```

`src/screens/AiBridge.test.tsx` - replace `the database card hides when switched off in Settings, except during the tour` (line ~806) and adapt `the Company database card leads with the connection, not the PHR-X server` (line ~832). Use the file's existing `renderBridge`, `DB_TOOLS` and mock shapes (`detect_ai_tools` rows carry `registered_servers`):
```ts
test("the connection is always shown; the PHR X option is not, by default", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") return DB_TOOLS; // none has phr-db-mcp registered
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  expect(await screen.findByText("Company database")).toBeInTheDocument();
  expect(screen.getByLabelText("Default connections")).toBeInTheDocument();
  expect(screen.queryByLabelText("Database server path")).not.toBeInTheDocument();
  expect(screen.queryByText(/no longer needed for lookups/)).not.toBeInTheDocument();
});

test("switched on in Settings, the PHR X option appears as before", async () => {
  localStorage.setItem("tcm-v2-ai-show-phrx", "on");
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") return DB_TOOLS;
  });
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
    if (cmd === "unregister_db_server") { calls.push((args as { id: string }).id); return null; }
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  expect(await screen.findByText(/still registered with the tools below/)).toBeInTheDocument();
  // No executable path configured, and still the row is there to remove it.
  expect(screen.queryByLabelText("Database server path")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Register$/ })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /Unregister/ }));
  await waitFor(() => expect(calls).toEqual(["claude-code"]));
});
```
Match the real row shape of `detect_ai_tools` (copy `DB_TOOLS`' fields exactly) and the real argument name of `unregister_db_server` (read `src/bindings.ts`). If the tour test that relied on the card showing only during the tour has nothing left to prove, delete it (the card is always shown now) and say so in the report.

- [ ] **Step 2: Run to verify failure**

`npx vitest run src/lib/aiScope.test.ts src/screens/Settings.test.tsx src/screens/AiBridge.test.tsx --exclude "**/.claude/**"` - the new tests fail (missing exports, old label, PHR X controls always rendered, leftover rows hidden without an executable path).

- [ ] **Step 3: Implement** per Interfaces. Keep the existing comments' voice; update the comment at `AiBridge.tsx:96-98` to say why the connection is always shown and the PHR X part is opt-in. Update the README as described.

- [ ] **Step 4: Run** the Step 2 command, then `npx vitest run src/App.test.tsx src/tour src/ui-consistency.test.ts src/a11y.test.tsx --exclude "**/.claude/**"`, then `npx tsc --noEmit`. All green.

- [ ] **Step 5: Commit**
```bash
git add src/lib/aiScope.ts src/lib/aiScope.test.ts src/screens/Settings.tsx src/screens/Settings.test.tsx src/screens/AiBridge.tsx src/screens/AiBridge.test.tsx README.md
git commit -q -F - <<'EOF'
feat(v2): the PHR X database server is an option you switch on, and a leftover registration can still be removed

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 2: "Your own database" in the connection picker

**Files:**
- Modify: `src/screens/AiBridge.tsx:640-673` (the `Default connections` Combobox) and the card's intro paragraph (`:620-626`)
- Test: `src/screens/AiBridge.test.tsx` (near `picking a preset fills and persists the connection`, line ~307)

**Interfaces:**
- Consumes: Task 1's always-rendered card; `editDb`, `setRawConn`, `db`, `dbPresets`, `isRepresentable`, `EMPTY_FIELDS`/`buildConnString` from `src/lib/connString.ts` as the file already uses them.
- `const OWN_DATABASE = "Your own database";` (module constant in `AiBridge.tsx`).
- The Combobox `options` become `[...preset labels, OWN_DATABASE]`. Its `value` is the matching preset's label, else `OWN_DATABASE` whenever the stored connection string is non-empty or the person picked it; it stays blank only for an empty, never-picked connection.
- Picking `OWN_DATABASE`: `editDb({ connection_string: "" })`, `setRawConn(false)` (so the Server host / Port / Database / User / Password fields show, empty), and focus the `Database host` input (the Server host field's `aria-label`). It does NOT push anything into a PHR X registration (unlike picking a preset, which syncs registered configs).
- A hint under the picker, verbatim: `Not listed? Choose "Your own database" and enter its server, database, user and password below. Connections you enter yourself are read only unless the user is a dev login.`
- Nothing else about saving changes: the fields already persist per edit through `editDb`, and App already pushes the connection to the bridge.

- [ ] **Step 1: Write the failing tests** (in `AiBridge.test.tsx`, using its `renderBridge` and mock shapes; `db_server_presets` returns the shipped presets in the existing preset test - copy that mock):
```ts
test("Your own database clears the connection for typing and stays selected for a hand-entered one", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") return DB_TOOLS;
    if (cmd === "db_server_presets") return PRESETS; // the fixture the preset test uses
    if (cmd === "db_server_defaults") return null;
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  fireEvent.click(await screen.findByLabelText("Default connections"));
  fireEvent.click(await screen.findByText("Your own database"));
  expect(screen.getByLabelText("Database host")).toHaveValue("");
  expect(screen.getByLabelText("Database name")).toHaveValue("");
  expect(screen.getByLabelText("Database host")).toHaveFocus();
  fireEvent.change(screen.getByLabelText("Database host"), { target: { value: "sql.example.local" } });
  fireEvent.change(screen.getByLabelText("Database name"), { target: { value: "Payroll" } });
  fireEvent.change(screen.getByLabelText("Database user"), { target: { value: "reader" } });
  fireEvent.change(screen.getByLabelText("Database password"), { target: { value: "s3cret" } });
  const stored = JSON.parse(localStorage.getItem("tcm-v2-db-mcp")!);
  expect(stored.connection_string).toContain("sql.example.local");
  expect(stored.connection_string).toContain("Payroll");
  expect(screen.getByLabelText("Default connections")).toHaveTextContent("Your own database");
  expect(screen.getByText(/Not listed\? Choose "Your own database"/)).toBeInTheDocument();
  localStorage.clear();
});

test("a stored connection that matches no preset shows as Your own database", async () => {
  localStorage.setItem("tcm-v2-db-mcp", JSON.stringify({
    exe_path: "", db_type: "mssql", schema_filter: "",
    connection_string: "Server=sql.example.local;Database=Payroll;User Id=reader;Password=p;",
  }));
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") return DB_TOOLS;
    if (cmd === "db_server_presets") return PRESETS;
  });
  renderBridge(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  expect(await screen.findByLabelText("Default connections")).toHaveTextContent("Your own database");
  localStorage.clear();
});
```
`PRESETS` is a module-level fixture you add, copied from the inline list in `picking a preset fills and persists the connection` (line ~311): two presets, `Dev — read only` and `QA — read only` (those em dashes are existing shipped labels, quoted as they are). Keep the existing preset test passing unchanged.

- [ ] **Step 2: Run to verify failure:** `npx vitest run src/screens/AiBridge.test.tsx --exclude "**/.claude/**"`.

- [ ] **Step 3: Implement** per Interfaces.

- [ ] **Step 4: Run** `npx vitest run src/screens/AiBridge.test.tsx src/ui-consistency.test.ts src/a11y.test.tsx --exclude "**/.claude/**"` and `npx tsc --noEmit`.

- [ ] **Step 5: Commit**
```bash
git add src/screens/AiBridge.tsx src/screens/AiBridge.test.tsx
git commit -q -F - <<'EOF'
feat(v2): the connection picker offers Your own database, for a server the app did not ship with

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

## After execution

Hand checks in a dev build: (1) fresh profile: AI Bridge shows the Company database connection and no PHR X controls; Settings shows the PHR X switch off. (2) Switch it on: the PHR X part is back. (3) With a tool that has `phr-db-mcp` registered and the switch off: the notice and an Unregister button show; Unregister removes it from that tool's config. (4) Choose "Your own database", type a server, database, user and password: the assistant's `db_lookup` reaches that database.

// Capture mode (the help site's screenshots) starts every boot from the
// same scene, so a shot does not depend on what an earlier route did, and
// shows the sample data under neutral names only.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  once: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(() => Promise.resolve()),
}));

const DRAFT = "tcm-v2-draft:Contoso/1001";

beforeEach(() => {
  vi.resetModules();
  localStorage.clear();
  window.location.hash = "";
});
afterEach(() => {
  localStorage.clear();
  window.location.hash = "";
});

async function boot({ demo, capture }: { demo: boolean; capture: boolean }) {
  if (demo) localStorage.setItem("tcm-v2-dev-demo", "on");
  if (capture) localStorage.setItem("tcm-v2-dev-capture", "on");
  const m = await import("./demo");
  m.maybeEnableDemoMode();
  const { commands } = await import("../bindings");
  return { ...m, commands: commands as unknown as Record<string, (...a: unknown[]) => Promise<unknown>> };
}

/** Every string inside a value. */
function strings(value: unknown, out: string[] = []): string[] {
  if (typeof value === "string") out.push(value);
  else if (Array.isArray(value)) value.forEach((v) => strings(v, out));
  else if (value && typeof value === "object") Object.values(value).forEach((v) => strings(v, out));
  return out;
}

/** Every number inside a value. */
function numbers(value: unknown, out: number[] = []): number[] {
  if (typeof value === "number") out.push(value);
  else if (Array.isArray(value)) value.forEach((v) => numbers(v, out));
  else if (value && typeof value === "object") Object.values(value).forEach((v) => numbers(v, out));
  return out;
}

test("in capture mode, every boot resets the PBI, the mode and the queue", async () => {
  // What an earlier shot left behind: another screen, Work Manager, no PBI,
  // and an uploaded (emptied) queue.
  localStorage.setItem(
    "tcm-v2-prefs",
    JSON.stringify({ org: "Contoso", project: "Customer Portal", section: "run", pbi: null, workMode: true }),
  );
  localStorage.setItem(DRAFT, "[]");
  const { CAPTURE_QUEUE } = await boot({ demo: true, capture: true });
  expect(JSON.parse(localStorage.getItem("tcm-v2-prefs")!)).toEqual({
    org: "Contoso",
    project: "Customer Portal",
    section: "manual",
    pbi: { id: 1001, title: "Login and session flow", work_item_type: "Product Backlog Item" },
    workMode: false,
  });
  expect(JSON.parse(localStorage.getItem("tcm-v2-recent-pbis:Contoso/Customer Portal")!)).toHaveLength(2);
  expect(JSON.parse(localStorage.getItem(DRAFT)!)).toEqual(CAPTURE_QUEUE);
  // The three states the queue documents: new, update, a duplicate title.
  expect(CAPTURE_QUEUE.map((c) => c.update_id)).toEqual([null, 5002, null]);
});

test("the runner window is left alone", async () => {
  window.location.hash = "#runner";
  await boot({ demo: true, capture: true });
  expect(localStorage.getItem(DRAFT)).toBeNull();
  expect(localStorage.getItem("tcm-v2-prefs")).toBeNull();
});

test("sample data without capture mode keeps whatever queue there is", async () => {
  localStorage.setItem(DRAFT, "[]");
  await boot({ demo: true, capture: false });
  expect(localStorage.getItem(DRAFT)).toBe("[]");
});

test("capture mode without sample data seeds nothing", async () => {
  await boot({ demo: false, capture: true });
  expect(localStorage.getItem(DRAFT)).toBeNull();
});

test("the capture names read as real names", async () => {
  const { neutralName } = await import("./demo");
  expect(neutralName("Demo - Login & session flow")).toBe("Login and session flow");
  expect(neutralName("DemoOrg/Demo Project")).toBe("Contoso/Customer Portal");
  expect(neutralName("@Demo User can you confirm the timeout on the demo build?")).toBe(
    "@Alex Tester can you confirm the timeout on the staging build?",
  );
  expect(neutralName("smoke; demo")).toBe("smoke; portal");
  expect(neutralName("Demo.InitialFindings DEMO")).toBe("Portal.InitialFindings PORTAL");
});

test("in capture mode, nothing a patched command answers says demo or was borrowed from a real org", async () => {
  const { commands, CAPTURE_QUEUE } = await boot({ demo: true, capture: true });
  // Every patched command, read the way demoCoverage.test.ts reads them.
  const names = [...readFileSync(resolve(__dirname, "demo.ts"), "utf8").matchAll(/^ {4}(\w+):/gm)].map((m) => m[1]);
  expect(names.length).toBeGreaterThan(60);
  const ids = [1001, 1002, 2001, 2002, 2003, 2004, 5001, 5002, 5101, 90, 91, 93, 95, 501, 502, 503, 700, 900, 901, 902];
  const argSets: unknown[][] = [[]];
  for (const id of ids) {
    argSets.push(["Contoso", "Customer Portal", id, id, id, id, id]);
    argSets.push(["Contoso", id, id, id]);
    argSets.push(["Contoso", "Customer Portal", "repo", id, id]);
  }
  argSets.push(["Contoso", ids], ["Contoso", "Customer Portal", ids], ["Contoso", "Customer Portal", "login"]);
  // Only the one that empties the store is left out, so the rest see it all.
  const skip = new Set(["deleteTestCases"]);
  const seen: string[] = [];
  const numbersSeen: number[] = [];
  let answered = 0;
  for (const name of names) {
    if (skip.has(name)) continue;
    for (const args of argSets) {
      try {
        const answer = await commands[name](...args);
        seen.push(...strings(answer));
        numbersSeen.push(...numbers(answer));
        answered++;
      } catch {
        // wrong argument shapes for this command - the others cover it
      }
    }
  }
  expect(answered).toBeGreaterThan(names.length);
  seen.push(...strings(CAPTURE_QUEUE), localStorage.getItem("tcm-v2-prefs") ?? "");
  expect(seen.filter((s) => /demo/i.test(s))).toEqual([]);
  // Nothing borrowed from a real organisation either: the people, titles
  // and test project the sample data took from real life (the left side of
  // CAPTURE_NAMES), and the real work item number the pull request links.
  const { CAPTURE_NAMES } = await import("./demo");
  const borrowed = CAPTURE_NAMES.map(([from]) => from).filter((from) => !/demo/i.test(from));
  expect(borrowed.length).toBeGreaterThan(5);
  expect(seen.filter((s) => borrowed.some((b) => s.includes(b)))).toEqual([]);
  expect(seen.filter((s) => s.includes("143783"))).toEqual([]);
  expect(numbersSeen).not.toContain(143783);
  expect(numbersSeen).toContain(2005);
  // And the names the shots rely on are the ones answered.
  expect(seen).toContain("Contoso");
  expect(seen).toContain("Customer Portal");
  expect(seen).toContain("Login and session flow");
});

test("outside capture mode the sample data keeps its own names", async () => {
  const { commands } = await boot({ demo: true, capture: false });
  expect(strings(await commands.listOrgs())).toContain("DemoOrg");
});

test("in capture mode, every boot clears remembered view choices and seeds one local comment", async () => {
  // What earlier shots left behind: grouping switched on, folded groups, a
  // run order of the tester's own and the runner unpinned.
  const left = {
    "tcm-v2-group-cases": "on",
    "tcm-v2-group-view": "on",
    "tcm-v2-group-manage": "on",
    "tcm-v2-group-mode": "title",
    "tcm-v2-run-collapsed-groups": '["Login"]',
    "tcm-v2-runner-pinned": "off",
    "tcm-v2-run-order:Contoso/90/91": "[5002,5001]",
    "tcm-v2-run-order-view:Contoso/90/91": "mine",
  };
  for (const [k, v] of Object.entries(left)) localStorage.setItem(k, v);
  localStorage.setItem("tcm-v2-theme-id", "slate");
  const { CAPTURE_NOTES } = await boot({ demo: true, capture: true });
  for (const k of Object.keys(left)) expect(localStorage.getItem(k)).toBeNull();
  // Anything not on the list is left alone.
  expect(localStorage.getItem("tcm-v2-theme-id")).toBe("slate");
  expect(JSON.parse(localStorage.getItem("tcm-v2-case-notes:Contoso")!)).toEqual(CAPTURE_NOTES);
  expect(strings(CAPTURE_NOTES).filter((s) => /demo/i.test(s))).toEqual([]);
});

test("the suites list their cases in capture mode only", async () => {
  const capture = await boot({ demo: true, capture: true });
  const entries = (await capture.commands.listSuiteEntries("Contoso", "Customer Portal", 91)) as {
    data: { id: number; entry_type: string }[];
  };
  expect(entries.data.map((e) => e.id)).toEqual([5001, 5002, 5003, 5004, 5005]);
  expect(entries.data.every((e) => e.entry_type === "testCase")).toBe(true);

  vi.resetModules();
  localStorage.clear();
  const plain = await boot({ demo: true, capture: false });
  expect(await plain.commands.listSuiteEntries("DemoOrg", "Demo Project", 91)).toEqual({ status: "ok", data: [] });
});

test("in capture mode, the AI Bridge, the board and pull requests start from the same scene", async () => {
  // What an earlier shot (or the person's own use) left behind.
  const left = {
    "tcm-v2-ai-global-allowed": "on",
    "tcm-v2-ai-scope": "global",
    "tcm-v2-ai-show-phrx": "on",
    "tcm-v2-mcp-disabled": '["get_run_failures"]',
    "tcm-v2-db-mcp": '{"exe_path":"C:\\\\real\\\\server.exe","db_type":"mssql","schema_filter":""}',
    "tcm-v2-db-writes": "1",
    "tcm-v2-working-dir": "C:\\real\\repo",
    "tcm-v2-board-swimlanes": "on",
    "tcm-v2-board-lanes-collapsed:Contoso/Customer Portal": "[1001]",
    "tcm-v2-hidden-cols": '["Done"]',
    "tcm-v2-type-filter": '["Bug"]',
    "tcm-v2-this-sprint": "on",
    "tcm-v2-pr-yours:Contoso/Customer Portal": "off",
    "tcm-v2-pr-status": "completed",
  };
  for (const [k, v] of Object.entries(left)) localStorage.setItem(k, v);
  localStorage.setItem("tcm-v2-repositories", JSON.stringify([{ path: "C:\\real\\repo", enabled: true }]));
  localStorage.setItem("tcm-v2-db-selected", "real-db");
  const { CAPTURE_REPO } = await boot({ demo: true, capture: true });
  for (const k of Object.keys(left)) expect(localStorage.getItem(k)).toBeNull();
  expect(JSON.parse(localStorage.getItem("tcm-v2-repositories")!)).toEqual([{ path: CAPTURE_REPO, enabled: true }]);
  expect(localStorage.getItem("tcm-v2-current-repo")).toBe(CAPTURE_REPO);
  expect(localStorage.getItem("tcm-v2-db-selected")).toBe("dev");
  // The sample repository, by the id the capture names answer listRepos with.
  expect(JSON.parse(localStorage.getItem("tcm-v2-pr-repos:Contoso/Customer Portal")!)).toEqual(["portal-repo-1"]);
});

test("in capture mode the AI Bridge shows sample tools and databases, never this machine's", async () => {
  const { commands, CAPTURE_DATABASES } = await boot({ demo: true, capture: true });
  const tools = (await commands.detectAiTools(null)) as { registered_servers: string[]; scope: string }[];
  expect(tools.map((t) => t.registered_servers)).toEqual([["tcm-testcases"], []]);
  expect(tools.every((t) => t.scope === "project")).toBe(true);
  expect(await commands.dbDatabases()).toEqual(CAPTURE_DATABASES);
  expect(await commands.dbServerDefaults()).toEqual({ exe_path: "", db_type: "mssql", schema_filter: "" });

  // The open thread's reply carries a screenshot, and commentImages answers it.
  const threads = (await commands.prThreads("Contoso", "Customer Portal", "portal-web", 501)) as {
    data: { comments: { content: string }[] }[];
  };
  const texts = threads.data.flatMap((t) => t.comments.map((c) => c.content));
  const images = (await commands.commentImages("Contoso", texts)) as { data: { url: string; data: string }[] };
  expect(images.data).toHaveLength(1);
  expect(texts.some((t) => t.includes(images.data[0].url))).toBe(true);
  expect(images.data[0].data).toMatch(/^data:image\/svg\+xml;base64,/);

  vi.resetModules();
  localStorage.clear();
  const plain = await boot({ demo: true, capture: false });
  // Normal sample data keeps its own two tools, with no registration detail.
  const plainTools = (await plain.commands.detectAiTools(null)) as { registered_servers?: string[] }[];
  expect(plainTools.map((t) => t.registered_servers)).toEqual([undefined, undefined]);
});

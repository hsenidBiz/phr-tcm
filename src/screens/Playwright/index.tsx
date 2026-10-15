// The Playwright tab: the team's PHR-PLAYWRIGHT-AUTOMATION repo, read from
// the clone folder Export to Playwright uses. Offered exactly where Auto Run
// is (lib/extras); Rust refuses its commands everywhere else.
//
// A header (the clone folder and its health badges), then four tabs:
// Overview (what the clone holds), the Runner (./Runner), the test accounts
// (./Accounts) and Sync (./Sync). Export to Playwright opens here, from Auto
// Run's menu item (lib/playwrightExport).

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useState, type ReactNode } from "react";
import { commands, type HealthIssue, type RepoStructure } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconBrowse, IconRefresh } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import {
  clearPlaywrightExportRequest,
  useClearExportRequestOnUnmount,
  usePlaywrightExportRequest,
} from "../../lib/playwrightExport";
import Accounts, { ACCOUNTS_KEY } from "./Accounts";
import HealthLine, { HEALTH_TEXT, WARNING_ONLY } from "./HealthLine";
import { Hint, STRUCTURE_KEY, Stat, SubTabs, type TabSpec } from "./parts";
import PlaywrightExportDialog from "./PlaywrightExportDialog";
import Runner, { REPORTS_KEY, TESTS_KEY } from "./Runner";
import Sync, { SYNC_KEY } from "./Sync";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export { STRUCTURE_KEY };

type Tab = "overview" | "runner" | "accounts" | "sync";
/** The tab last chosen, kept for the session: coming back to Playwright
 * opens where the person left it. */
let lastTab: Tab = "overview";

function Overview({ s }: { s: RepoStructure }) {
  const specs = s.features.reduce((n, f) => n + f.spec_count, 0);
  const cases = s.features.reduce((n, f) => n + f.case_count, 0);
  const th = "px-3 py-2 text-left text-xs font-medium text-muted";
  const num = "px-3 py-2 text-right tabular-nums";
  return (
    <section className="space-y-4" aria-label="Repo structure">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 xl:grid-cols-6">
        <Stat value={s.features.length} label={s.features.length === 1 ? "feature" : "features"} />
        <Stat value={specs} label={specs === 1 ? "spec" : "specs"} />
        <Stat value={cases} label={cases === 1 ? "test case" : "test cases"} />
        <Stat value={s.generated_ids.length} label={s.generated_ids.length === 1 ? "raw spec" : "raw specs"} />
        <Stat value={s.user_keys.length} label={s.user_keys.length === 1 ? "account" : "accounts"} />
        <Stat
          value={s.navigation_keys.length}
          label={s.navigation_keys.length === 1 ? "navigation entry" : "navigation entries"}
        />
      </div>
      {s.features.length === 0 ? (
        <Hint>No features under suites yet.</Hint>
      ) : (
        <div className="overflow-x-auto rounded-lg border border-border bg-surface">
          <table className="w-full text-sm" aria-label="Features">
            <thead className="border-b border-border bg-surface-2">
              <tr>
                <th className={th}>Folder</th>
                <th className={th}>Feature</th>
                <th className={`${th} text-right`}>Specs</th>
                <th className={`${th} text-right`}>Cases</th>
              </tr>
            </thead>
            <tbody>
              {s.features.map((f) => (
                <tr key={f.path.join("/")} className="border-b border-border last:border-b-0">
                  <td className="px-3 py-2 text-muted">{f.path.slice(0, -1).join(" / ")}</td>
                  <td className="px-3 py-2 font-medium text-text">{f.path[f.path.length - 1]}</td>
                  <td className={`${num} text-text`}>{f.spec_count}</td>
                  <td className={`${num} ${f.case_count ? "text-text" : "text-faint"}`}>{f.case_count}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

/** In place of an area the clone cannot serve yet: why, and what to do. */
function Blocked({ area, issues }: { area: string; issues: HealthIssue[] }) {
  const blocking = issues.filter((i) => !WARNING_ONLY.has(i));
  return (
    <div className="rounded-lg border border-dashed border-border bg-surface px-4 py-6 text-center">
      <p className="text-sm font-medium text-text">{`The ${area} needs a working clone.`}</p>
      {blocking.map((i) => (
        <p key={i} className="mt-1 text-xs text-danger">
          {HEALTH_TEXT[i]}
        </p>
      ))}
    </div>
  );
}

export default function Playwright({ org, project }: { org: string; project: string }) {
  const qc = useQueryClient();
  const exportRequest = usePlaywrightExportRequest();
  useClearExportRequestOnUnmount();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  /** A Runner or Accounts command (or an account change) is going: Refresh
   * waits for it rather than read the clone in the middle of it. */
  const [runnerBusy, setRunnerBusy] = useState(false);
  const [accountsBusy, setAccountsBusy] = useState(false);
  const onRunnerBusy = useCallback((b: boolean) => setRunnerBusy(b), []);
  const onAccountsBusy = useCallback((b: boolean) => setAccountsBusy(b), []);
  const [tab, setTabState] = useState<Tab>(lastTab);
  const setTab = (t: Tab) => {
    lastTab = t;
    setTabState(t);
  };

  const settings = useQuery({ queryKey: ["app-settings"], queryFn: () => commands.getAppSettings() });
  const clone = settings.data?.playwright_clone ?? "";
  const structure = useQuery({
    queryKey: [STRUCTURE_KEY, clone],
    queryFn: () => unwrapStr(commands.pwRepoStructure(clone)),
    enabled: settings.isSuccess && clone !== "",
    // Kept until something says it changed: Refresh, a change made here
    // (`changed`), or an export (`closeExport`).
    staleTime: Infinity,
  });

  const choose = async () => {
    setBusy(true);
    setError("");
    try {
      const picked = await open({ directory: true });
      if (typeof picked !== "string" || !picked) return;
      const s = await unwrapStr(commands.setPlaywrightClone(picked));
      qc.setQueryData(["app-settings"], s);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const closeExport = () => {
    clearPlaywrightExportRequest();
    // The dialog can change the clone folder, and an export changes the clone.
    void qc.invalidateQueries({ queryKey: ["app-settings"] });
    void qc.invalidateQueries({ queryKey: [STRUCTURE_KEY] });
    void qc.invalidateQueries({ queryKey: [SYNC_KEY] });
    void qc.invalidateQueries({ queryKey: [TESTS_KEY] });
  };

  const s = structure.data;
  const usable = s && !s.health.includes("NotTheRepo");
  // Nothing can run without the clone's node_modules and Node: the health
  // badges say so, and the Runner and Accounts show why instead.
  const runnable = usable && !s.health.includes("NoNodeModules") && !s.health.includes("NoNode");
  /** The clone was written here (an account change, a Sync write): what it
   * holds, and its tests, are read again. */
  const changed = () => {
    void qc.invalidateQueries({ queryKey: [STRUCTURE_KEY, clone] });
    void qc.invalidateQueries({ queryKey: [TESTS_KEY, clone] });
  };
  /** Refresh: every Playwright query is read again, the shown ones now. */
  const refreshAll = () => {
    for (const k of [STRUCTURE_KEY, TESTS_KEY, REPORTS_KEY, ACCOUNTS_KEY, SYNC_KEY]) void qc.invalidateQueries({ queryKey: [k] });
  };

  /** About to open the Runner: its listing starts now. Only when there is
   * none yet: a listing already there, or one that failed (a repo command
   * that failed, which List again retries), is not asked for here. */
  const intent = (t: Tab) => {
    if (t !== "runner" || !runnable) return;
    const now = qc.getQueryState([TESTS_KEY, clone]);
    if (now && (now.data !== undefined || now.error !== null || now.fetchStatus === "fetching")) return;
    void qc.prefetchQuery({
      queryKey: [TESTS_KEY, clone],
      queryFn: () => unwrapStr(commands.pwList(clone, false)),
      retry: false,
      staleTime: Infinity,
    });
  };

  const tabs: TabSpec<Tab>[] = [
    { id: "overview", label: "Overview" },
    { id: "runner", label: "Runner" },
    { id: "accounts", label: "Accounts", count: s?.user_keys.length },
    { id: "sync", label: "Sync" },
  ];
  const panels: Record<Tab, ReactNode> = {
    overview: s && <Overview s={s} />,
    runner: runnable ? <Runner key={clone} clone={clone} onBusy={onRunnerBusy} /> : s && <Blocked area="Runner" issues={s.health} />,
    accounts: runnable ? (
      <Accounts key={clone} clone={clone} onChanged={changed} onBusy={onAccountsBusy} />
    ) : (
      s && <Blocked area="Accounts tab" issues={s.health} />
    ),
    sync: <Sync key={clone} clone={clone} org={org} project={project} onUpdated={changed} />,
  };

  return (
    // Fills the main area (App lays this tab out like the board): the
    // section tabs take what the header leaves, and scroll inside.
    <div className="flex min-h-0 flex-1 flex-col gap-5">
      <header className="shrink-0 space-y-2">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <h2 className="text-lg font-semibold text-text">Playwright</h2>
          <div className="flex min-w-0 flex-1 basis-64 items-center gap-2 rounded-md border border-border bg-surface px-2.5 py-1.5">
            <IconBrowse aria-hidden className="size-4 shrink-0 text-muted" />
            <span className="min-w-0 flex-1 truncate font-mono text-xs text-text" title={clone || undefined}>
              {settings.isSuccess ? clone || "No folder chosen yet." : "Loading…"}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <Button size="sm" variant="outline" disabled={busy || !clone || structure.isFetching || runnerBusy || accountsBusy}
              onClick={refreshAll}>
              <IconRefresh aria-hidden />
              Refresh
            </Button>
            <Button size="sm" variant="outline" disabled={busy} onClick={() => void choose()}>
              <IconBrowse aria-hidden />
              {clone ? "Change…" : "Choose…"}
            </Button>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {s && <HealthLine issues={s.health} />}
          {structure.isLoading && <Hint>Reading the clone…</Hint>}
          {settings.isSuccess && !clone && <Hint>Choose your clone of the team's PHR-PLAYWRIGHT-AUTOMATION repo.</Hint>}
        </div>
        {error && <p className="text-xs text-danger">{error}</p>}
        {structure.isError && <p className="text-xs text-danger">{structure.error.message}</p>}
      </header>

      {s && !usable && (
        <div className="rounded-lg border border-dashed border-border bg-surface px-4 py-8 text-center">
          <p className="text-sm font-medium text-text">{HEALTH_TEXT.NotTheRepo}</p>
          <Hint className="mt-1">Press Change… and pick your clone of the PHR-PLAYWRIGHT-AUTOMATION repo.</Hint>
        </div>
      )}

      {usable && <SubTabs label="Playwright sections" tabs={tabs} shown={tab} onShow={setTab} panels={panels} onIntent={intent} />}

      {exportRequest && (
        <PlaywrightExportDialog
          org={org}
          project={project}
          pbiId={exportRequest.pbiId}
          caseIds={exportRequest.caseIds}
          modules={exportRequest.modules}
          onClose={closeExport}
        />
      )}
    </div>
  );
}

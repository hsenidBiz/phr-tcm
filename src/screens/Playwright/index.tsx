// The Playwright tab: the team's PHR-PLAYWRIGHT-AUTOMATION repo, read from
// the clone folder Export to Playwright uses. Offered exactly where Auto Run
// is (lib/extras); Rust refuses its commands everywhere else.
//
// Top to bottom: the clone folder, the health line, what the clone holds,
// the Runner (./Runner) and the test accounts (./Accounts). Export to Playwright opens here, from Auto Run's menu item
// (lib/playwrightExport).

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { commands, type RepoFeature, type RepoStructure } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconBrowse, IconRefresh } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import {
  clearPlaywrightExportRequest,
  useClearExportRequestOnUnmount,
  usePlaywrightExportRequest,
} from "../../lib/playwrightExport";
import Accounts from "./Accounts";
import HealthLine from "./HealthLine";
import PlaywrightExportDialog from "./PlaywrightExportDialog";
import Runner from "./Runner";

const sectionTitle = "text-xs font-semibold uppercase tracking-wide text-muted";
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** The query key of the clone's structure; the folder is part of it. */
export const STRUCTURE_KEY = "pw-repo-structure";

function Summary({ s }: { s: RepoStructure }) {
  const specs = s.features.reduce((n, f) => n + f.spec_count, 0);
  const cases = s.features.reduce((n, f) => n + f.case_count, 0);
  return (
    <p className="text-xs text-muted">
      {[
        plural(s.features.length, "feature"),
        plural(specs, "spec"),
        plural(cases, "test case"),
        plural(s.generated_ids.length, "raw spec"),
        plural(s.user_keys.length, "account"),
        plural(s.navigation_keys.length, "navigation entry", "navigation entries"),
      ].join(" · ")}
    </p>
  );
}

function FeatureRow({ f }: { f: RepoFeature }) {
  return (
    <li className="flex flex-wrap items-baseline gap-x-3 text-sm">
      <span className="text-text">{`${f.country} / ${f.side} / ${f.module} / ${f.feature}`}</span>
      <span className="text-xs text-muted">{`${plural(f.spec_count, "spec")}, ${plural(f.case_count, "test case")}`}</span>
    </li>
  );
}

export default function Playwright({ org, project }: { org: string; project: string }) {
  const qc = useQueryClient();
  const exportRequest = usePlaywrightExportRequest();
  useClearExportRequestOnUnmount();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const settings = useQuery({ queryKey: ["app-settings"], queryFn: () => commands.getAppSettings() });
  const clone = settings.data?.playwright_clone ?? "";
  const structure = useQuery({
    queryKey: [STRUCTURE_KEY, clone],
    queryFn: () => unwrapStr(commands.pwRepoStructure(clone)),
    enabled: settings.isSuccess && clone !== "",
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
  };

  const s = structure.data;
  const usable = s && !s.health.includes("NotTheRepo");
  // Nothing can run without the clone's node_modules and Node: the health
  // line says so, and the Runner is not offered.
  const runnable = usable && !s.health.includes("NoNodeModules") && !s.health.includes("NoNode");

  return (
    <div className="space-y-4">
      <section className="space-y-1">
        <h3 className={sectionTitle}>Clone folder</h3>
        <div className="flex flex-wrap items-center gap-2">
          <span className="min-w-0 flex-1 break-all text-sm text-text">
            {settings.isSuccess ? clone || "No folder chosen yet." : "Loading…"}
          </span>
          <Button size="sm" variant="outline" disabled={busy || !clone || structure.isFetching} onClick={() => void structure.refetch()}>
            <IconRefresh aria-hidden />
            Refresh
          </Button>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => void choose()}>
            <IconBrowse aria-hidden />
            {clone ? "Change…" : "Choose…"}
          </Button>
        </div>
        {error && <p className="text-xs text-danger">{error}</p>}
        {settings.isSuccess && !clone && (
          <p className="text-xs text-muted">Choose your clone of the team's PHR-PLAYWRIGHT-AUTOMATION repo.</p>
        )}
        {structure.isLoading && <p className="text-xs text-muted">Reading the clone…</p>}
        {structure.isError && <p className="text-xs text-danger">{structure.error.message}</p>}
        {s && <HealthLine issues={s.health} />}
      </section>

      {usable && (
        <section className="space-y-2" aria-label="Repo structure">
          <h3 className={sectionTitle}>In the repo</h3>
          <Summary s={s} />
          {s.features.length === 0 ? (
            <p className="text-xs text-muted">No features under suites yet.</p>
          ) : (
            <ul className="space-y-1">
              {s.features.map((f) => (
                <FeatureRow key={`${f.country}/${f.side}/${f.module}/${f.feature}`} f={f} />
              ))}
            </ul>
          )}
        </section>
      )}

      {runnable && <Runner key={clone} clone={clone} />}

      {runnable && (
        <Accounts key={clone} clone={clone} onChanged={() => void qc.invalidateQueries({ queryKey: [STRUCTURE_KEY, clone] })} />
      )}

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

// Export to Playwright: copies this PBI's passing Auto Run scripts into a
// local clone of the team's PHR-PLAYWRIGHT-AUTOMATION repo, as raw specs plus
// a section in the area's test-case file. TCM only writes files; running,
// refactoring and committing them is the person's (or Claude Code's) job in
// the clone, which the summary says in order; Finish in the repo gives the
// prompt that has Claude Code there run the repo's own last steps.
//
// Four parts, top to bottom: the clone folder, where each area's specs live,
// which repo user each of TCM's accounts signs in as, and the cases. The
// preview (what each case needs, and why one cannot go) is read again after
// a mapping is saved or the clone changes, so the reasons stay current.
//
// Each case says whether the repo already has it (pw_repo::presence): a new
// case is ticked, one in the repo is not and is skipped. Ticking one sends it
// as chosen to replace; the write refuses a case the repo has otherwise.

import { useCallback, useEffect, useId, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  commands,
  type ExportMap,
  type ExportResult,
  type Placement,
  type Preview,
  type PreviewCase,
} from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Input } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { MODAL_LARGE } from "../AutoRun/modalWidths";
import { IconBrowse, IconCancel, IconConfirm, IconCopy, IconExport } from "../../lib/actionIcons";
import { copyText } from "../../lib/clipboard";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { unwrapStr } from "../../lib/ipc";
import FinishInRepo from "./FinishInRepo";

const SIDES = ["admin", "self"];
const sectionTitle = "text-xs font-semibold uppercase tracking-wide text-muted";
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

const NEXT_STEPS = [
  "Capture navigation for any area listed above as missing.",
  "Run each raw spec in the generated project, with the seed set to the case's user key.",
  "Paste the Finish in the repo prompt below into Claude Code in your clone: it runs the repo's test-refactorer, npm run lint:tests -- --require-specs and the live check.",
  "Review the changes and commit them.",
];

/** What the dialog says about cases the repo already has. */
export const SKIP_NOTE = "Cases already in the repo are skipped. Tick one to replace it.";

/** "New", or "In the repo" with where, and whether this app put it there. */
export function repoStatus(c: Pick<PreviewCase, "in_repo" | "repo_note" | "from_app"> & { excluded?: boolean }): string {
  if (c.excluded) return c.repo_note ?? "Excluded in the repo";
  if (!c.in_repo) return "New";
  const where = c.repo_note ? `: ${c.repo_note}` : "";
  return `In the repo${where}${c.from_app ? " (exported by this app)" : ""}`;
}

export default function PlaywrightExportDialog({
  org,
  project,
  pbiId,
  caseIds,
  modules,
  onClose,
}: {
  org: string;
  project: string;
  pbiId: number;
  caseIds: number[];
  /** Each case's Module: Auto Run picks the area of a script that names none by it. */
  modules: [number, string][];
  onClose: () => void;
}) {
  const titleId = useId();
  const { prefs } = useFieldRefs(org, project);
  const [clone, setClone] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  /**
   * The area rows and account picks being edited; seeded from the saved map,
   * with a suggestion for each area not placed yet (unsaved until confirmed).
   */
  const [areas, setAreas] = useState<Record<string, Placement> | null>(null);
  const [users, setUsers] = useState<Record<string, string> | null>(null);
  /** New exportable cases the person unticked; every other new one is ticked. */
  const [unticked, setUnticked] = useState<Set<number>>(new Set());
  /** Exportable cases the repo has that the person ticked to replace. */
  const [replacing, setReplacing] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<ExportResult | null>(null);
  /** A preview that failed after a successful write: secondary to the summary. */
  const [refreshError, setRefreshError] = useState("");

  /** Reads the preview again. `reseed` puts the rows back to what is saved. */
  const refresh = useCallback(async (reseed = true) => {
    const p = await unwrapStr(commands.pwExportPreview(org, project, caseIds, modules));
    setPreview(p);
    if (reseed) {
      setAreas({ ...(p.suggested ?? {}), ...(p.map.areas ?? {}) });
      setUsers({ ...(p.map.accounts?.[p.environment] ?? {}) });
    }
  }, [org, project, caseIds, modules]);

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const s = await commands.getAppSettings();
        if (live) setClone(s.playwright_clone ?? "");
        await refresh();
      } catch (e) {
        if (live) setError(message(e));
      }
    })();
    return () => {
      live = false;
    };
    // Mounted fresh on every open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** Runs `work`, showing a refusal under the buttons instead of closing. */
  const attempt = async (work: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try {
      await work();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const choose = () =>
    attempt(async () => {
      const picked = await open({ directory: true });
      if (typeof picked !== "string" || !picked) return;
      const s = await unwrapStr(commands.setPlaywrightClone(picked));
      setClone(s.playwright_clone ?? picked);
      await refresh();
    });

  const saveMappings = () =>
    attempt(async () => {
      if (!preview || invalid.size > 0) return;
      const map: ExportMap = {
        areas: draftAreas,
        accounts: { ...(preview.map.accounts ?? {}), [preview.environment]: draftUsers },
      };
      await unwrapStr(commands.pwExportSaveMap(org, project, map));
      await refresh();
    });

  // What the rows hold, as it would be saved, against what is saved. A row
  // with one of module/feature blank, or blanked over a saved mapping, is
  // invalid: it blocks the save instead of dropping the mapping.
  const savedAreas = preview?.map.areas ?? {};
  const savedUsers = preview?.map.accounts?.[preview.environment] ?? {};
  const invalid = new Set<string>();
  const draftAreas: Record<string, Placement> = {};
  for (const [name, p] of Object.entries(areas ?? {})) {
    const filled = Boolean(p.module.trim()) && Boolean(p.feature.trim());
    const touched = Boolean(p.module.trim()) || Boolean(p.feature.trim());
    if (filled) draftAreas[name] = { side: p.side, module: p.module.trim(), feature: p.feature.trim() };
    else if (touched || name in savedAreas) invalid.add(name);
  }
  const draftUsers = Object.fromEntries(Object.entries(users ?? {}).filter(([, v]) => v));
  const unsaved =
    invalid.size > 0 ||
    JSON.stringify(Object.entries(draftAreas).sort()) !== JSON.stringify(Object.entries(savedAreas).sort()) ||
    JSON.stringify(Object.entries(draftUsers).sort()) !== JSON.stringify(Object.entries(savedUsers).sort());

  const cases = preview?.cases ?? [];
  const isTicked = (c: PreviewCase) => (c.in_repo ? replacing.has(c.case_id) : !unticked.has(c.case_id));
  const ticked = cases.filter((c) => c.exportable && isTicked(c)).map((c) => c.case_id);
  const replace = cases.filter((c) => c.exportable && c.in_repo && replacing.has(c.case_id)).map((c) => c.case_id);
  const setTick = (c: PreviewCase, on: boolean) => {
    const flip = (cur: Set<number>, add: boolean) => {
      const next = new Set(cur);
      if (add) next.add(c.case_id);
      else next.delete(c.case_id);
      return next;
    };
    if (c.in_repo) setReplacing((cur) => flip(cur, on));
    else setUnticked((cur) => flip(cur, !on));
  };
  const canExport = Boolean(preview?.clone_ok) && ticked.length > 0 && !busy && !unsaved;

  const doExport = () =>
    attempt(async () => {
      setRefreshError("");
      const written = await unwrapStr(
        commands.pwExportWrite(org, project, ticked, modules, replace, prefs.moduleRef ?? null, prefs.preconditionsRef ?? null),
      );
      setResult(written);
      // What was just written is in the repo now: skipped by default again.
      setReplacing(new Set());
      try {
        await refresh(false);
      } catch (e) {
        setRefreshError(`The export was written, but the list could not be refreshed: ${message(e)}`);
      }
    });

  const place = (name: string): Placement => areas?.[name] ?? { side: "admin", module: "", feature: "" };
  const setPlace = (name: string, patch: Partial<Placement>) =>
    setAreas((cur) => ({ ...(cur ?? {}), [name]: { ...place(name), ...patch } }));

  return (
    <Modal onClose={() => !busy && onClose()} labelledBy={titleId} className={`${MODAL_LARGE} flex flex-col gap-3 p-4`}>
      <h2 id={titleId} className="text-sm font-semibold text-text">Export to Playwright</h2>
      <p className="text-xs text-muted">
        Copies the passing scripts of PBI {pbiId} into your clone of the Playwright repo. Nothing is run,
        committed or pushed.
      </p>
      {error && <p className="text-xs text-danger">{error}</p>}

      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto">
        <section className="space-y-1">
          <h3 className={sectionTitle}>Clone folder</h3>
          <div className="flex items-center gap-2">
            <span className="min-w-0 flex-1 break-all text-sm text-text">{clone || "No folder chosen yet."}</span>
            <Button size="sm" variant="outline" disabled={busy} onClick={() => void choose()}>
              <IconBrowse aria-hidden />
              Choose…
            </Button>
          </div>
          {preview && !preview.clone_ok && preview.clone_problem && (
            <p className="text-xs text-danger">{preview.clone_problem}</p>
          )}
        </section>

        {preview && areas && users && (
          <>
            <section className="space-y-2">
              <h3 className={sectionTitle}>Areas</h3>
              {preview.areas.length === 0 && <p className="text-xs text-muted">No areas yet.</p>}
              {preview.areas.map((name) => (
                <div key={name} className="flex flex-wrap items-center gap-2">
                  <span className="w-48 text-sm text-text">{name}</span>
                  <Select
                    aria-label={`Side for ${name}`}
                    className="w-28"
                    value={place(name).side}
                    onChange={(e) => setPlace(name, { side: e.target.value })}
                  >
                    {SIDES.map((s) => (
                      <option key={s} value={s}>{s}</option>
                    ))}
                  </Select>
                  <Input
                    aria-label={`Module for ${name}`}
                    placeholder="module"
                    className="w-44"
                    value={place(name).module}
                    onChange={(e) => setPlace(name, { module: e.target.value })}
                  />
                  <Input
                    aria-label={`Feature for ${name}`}
                    placeholder="feature"
                    className="w-44"
                    value={place(name).feature}
                    onChange={(e) => setPlace(name, { feature: e.target.value })}
                  />
                  {invalid.has(name) && <span className="text-xs text-danger">needs module and feature</span>}
                </div>
              ))}
            </section>

            <section className="space-y-2">
              <h3 className={sectionTitle}>Accounts</h3>
              {preview.accounts.length === 0 && <p className="text-xs text-muted">No accounts yet.</p>}
              {preview.accounts.map((key) => {
                // A saved pick the clone no longer has: shown, and said, not left blank.
                const stale = Boolean(users[key]) && !preview.user_keys.includes(users[key]);
                return (
                  <div key={key} className="flex flex-wrap items-center gap-2">
                    <span className="w-48 text-sm text-text">{key}</span>
                    <Select
                      aria-label={`User for ${key}`}
                      className="w-64"
                      value={users[key] ?? ""}
                      onChange={(e) => setUsers((cur) => ({ ...(cur ?? {}), [key]: e.target.value }))}
                    >
                      <option value="">Not mapped</option>
                      {stale && <option value={users[key]}>{users[key]}</option>}
                      {preview.user_keys.map((k) => (
                        <option key={k} value={k}>{k}</option>
                      ))}
                    </Select>
                    {stale && (
                      <span className="text-xs text-warning">{`${users[key]} is not in this clone's users.json`}</span>
                    )}
                  </div>
                );
              })}
            </section>

            <div className="flex items-center justify-end gap-2">
              {unsaved && <span className="text-xs text-warning">Save the mappings first</span>}
              <Button size="sm" variant="outline" disabled={busy || invalid.size > 0} onClick={() => void saveMappings()}>
                <IconConfirm aria-hidden />
                Save mappings
              </Button>
            </div>

            <section className="space-y-2">
              <h3 className={sectionTitle}>Cases</h3>
              <p className="text-xs text-muted">{SKIP_NOTE}</p>
              {cases.map((c) =>
                c.exportable ? (
                  <label key={c.case_id} className="flex flex-wrap items-center gap-2 text-sm text-text">
                    <Checkbox
                      checked={isTicked(c)}
                      ariaLabel={`#${c.case_id} ${c.title}`}
                      onCheckedChange={(on) => setTick(c, on)}
                    />
                    <span>{`#${c.case_id} ${c.title}`}</span>
                    <span className={`text-xs ${c.in_repo ? "text-warning" : "text-success"}`}>{repoStatus(c)}</span>
                  </label>
                ) : c.excluded ? (
                  // The team left it out of the repo on purpose: never exported.
                  <label key={c.case_id} className="flex flex-wrap items-center gap-2 text-sm text-faint">
                    <Checkbox checked={false} disabled ariaLabel={`#${c.case_id} ${c.title}`} onCheckedChange={() => {}} />
                    <span>{`#${c.case_id} ${c.title}`}</span>
                    <span className="text-xs">{repoStatus(c)}</span>
                  </label>
                ) : (
                  <div key={c.case_id} className="space-y-1 text-sm text-faint">
                    <p>{`#${c.case_id} ${c.title}`}</p>
                    {c.in_repo && <p className="text-xs">{repoStatus(c)}</p>}
                    {c.reason && <p className="text-xs">{c.reason}</p>}
                    {c.add_user_command && (
                      <div className="flex items-center gap-2">
                        <code className="rounded-md border border-border bg-surface-2 px-2 py-1 text-xs text-text">
                          {c.add_user_command}
                        </code>
                        <Button
                          size="sm"
                          variant="ghost"
                          aria-label={`Copy command for #${c.case_id}`}
                          onClick={() => void copyText(c.add_user_command ?? "")}
                        >
                          <IconCopy aria-hidden />
                          Copy
                        </Button>
                      </div>
                    )}
                  </div>
                ),
              )}
            </section>
          </>
        )}

        {result && (
          <section className="space-y-2 border-t border-border pt-3" aria-label="Export summary">
            <h3 className={sectionTitle}>Exported</h3>
            <ul className="space-y-0.5 text-xs text-text">
              {result.files.map((f) => (
                <li key={f}>{f}</li>
              ))}
            </ul>
            <ul className="space-y-0.5 text-xs text-muted">
              {result.cases.map(([id, file]) => {
                const user = result.user_keys.find(([u]) => u === id)?.[1];
                return <li key={id}>{user ? `#${id} → ${file} - run as ${user}` : `#${id} → ${file}`}</li>;
              })}
            </ul>
            {result.missing_navigation.length > 0 && (
              <div className="space-y-0.5 text-xs text-warning">
                <p>The clone's navigation.json has no entry for:</p>
                <ul>
                  {result.missing_navigation.map((m) => (
                    <li key={m}>{m}</li>
                  ))}
                </ul>
              </div>
            )}
            <p className="text-xs text-muted">Next:</p>
            <ol className="list-decimal space-y-0.5 pl-5 text-xs text-muted">
              {NEXT_STEPS.map((s) => (
                <li key={s}>{s}</li>
              ))}
            </ol>
            <p className="text-xs text-muted">
              Exporting a case again rewrites its section from Azure DevOps; the repo's Added by engineer, Also and Assertion floor blocks and every other section and file stay as they are.
            </p>
            {result.record_error && <p className="text-xs text-warning">{result.record_error}</p>}
            {refreshError && <p className="text-xs text-faint">{refreshError}</p>}
          </section>
        )}
        {result && <FinishInRepo clone={clone} org={org} project={project} result={result} />}
      </div>

      <div className="flex items-center justify-end gap-2">
        <Button variant="ghost" size="sm" disabled={busy} onClick={onClose}>
          <IconCancel aria-hidden />
          {result ? "Close" : "Cancel"}
        </Button>
        <Button size="sm" disabled={!canExport} onClick={() => void doExport()}>
          <IconExport aria-hidden />
          Export
        </Button>
      </div>
    </Modal>
  );
}

// Export to Playwright: copies this PBI's passing Auto Run scripts into a
// local clone of the team's PHR-PLAYWRIGHT-AUTOMATION repo, as raw specs plus
// a section in the area's test-case file. TCM only writes files; running,
// refactoring and committing them is the person's (or Claude Code's) job in
// the clone, which the summary says in order; Finish in the repo gives the
// prompt that has Claude Code there run the repo's own last steps.
//
// The clone folder at the top; then a summary strip, the cases grouped by
// Module and, beside them, the mappings: where each area's specs live and
// which repo user each of TCM's accounts signs in as. Cancel and Export stay
// in a footer while the body scrolls. A case with no Auto Run script is not
// listed at all: there is nothing of it to export. The
// preview (what each case needs, and why one cannot go) is read again after
// a mapping is saved or the clone changes, so the reasons stay current.
//
// Each case says whether the repo already has it (pw_repo::presence): a new
// case is ticked, one in the repo is not and is skipped. Ticking one sends it
// as chosen to replace; the write refuses a case the repo has otherwise.
//
// A case goes to its area's folder unless it is given its own: its row shows
// where it goes, and Own folder lets one case of an area sit apart from the
// rest. Own folders are saved with the other mappings. When a case's title
// names one of the clone's folders beside (or inside) its area's, the row
// suggests it (folderSuggest); taking it only edits the draft.

import { useCloseOnEnvironmentChange } from "../../hooks/useCloseOnEnvironmentChange";
import { useCallback, useEffect, useId, useState } from "react";
import { useQuery } from "@tanstack/react-query";
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
import Combobox from "../../components/ui/combobox";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { MODAL_LARGE } from "../AutoRun/modalWidths";
import { IconBrowse, IconCancel, IconConfirm, IconCopy, IconEdit, IconExport, IconUndo, IconWarning } from "../../lib/actionIcons";
import { copyText } from "../../lib/clipboard";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { unwrapStr } from "../../lib/ipc";
import FinishInRepo from "./FinishInRepo";
import { suggestFolder } from "./folderSuggest";
import NewFolder from "./NewFolder";
import { Card, Hint, STRUCTURE_KEY, Stat, StatusBadge } from "./parts";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

const NEXT_STEPS = [
  "Capture navigation for any area listed above as missing.",
  "Paste the Finish in the repo prompt below into Claude Code in your clone: it runs the raw specs, the repo's test-refactorer, npm run lint:tests -- --require-specs and the live check.",
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
  // What it holds is this environment's: it never outlives a switch.
  useCloseOnEnvironmentChange(onClose);
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
  /** TCM component name to the repo common component it stands for, as being edited. */
  const [comps, setComps] = useState<Record<string, string> | null>(null);
  /** Cases given their own folder: case id to the path as typed. */
  const [own, setOwn] = useState<Record<number, string>>({});
  /** New exportable cases the person unticked; every other new one is ticked. */
  const [unticked, setUnticked] = useState<Set<number>>(new Set());
  /** Exportable cases the repo has that the person ticked to replace. */
  const [replacing, setReplacing] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<ExportResult | null>(null);
  /** A preview that failed after a successful write: secondary to the summary. */
  const [refreshError, setRefreshError] = useState("");

  // The clone's own folders, offered as you type. The Overview's query, so
  // an open Playwright tab has them already and nothing is cached twice.
  const structure = useQuery({
    queryKey: [STRUCTURE_KEY, clone],
    queryFn: () => unwrapStr(commands.pwRepoStructure(clone)),
    enabled: clone !== "",
    staleTime: Infinity,
  });
  const featurePaths = (structure.data?.features ?? []).map((f) => f.path.join("/"));

  /** Reads the preview again. `reseed` puts the rows back to what is saved. */
  const refresh = useCallback(async (reseed = true) => {
    const p = await unwrapStr(commands.pwExportPreview(org, project, caseIds, modules));
    setPreview(p);
    if (reseed) {
      setAreas({ ...(p.suggested ?? {}), ...(p.map.areas ?? {}) });
      setUsers({ ...(p.map.accounts?.[p.environment] ?? {}) });
      setComps({ ...(p.map.components ?? {}) });
      setOwn(Object.fromEntries(Object.entries(p.map.cases ?? {}).map(([id, pl]) => [Number(id), pl?.path ?? ""])));
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
      if (!preview || invalid.size > 0 || invalidOwn.size > 0) return;
      const map: ExportMap = {
        areas: draftAreas,
        cases: draftCases,
        accounts: { ...(preview.map.accounts ?? {}), [preview.environment]: draftUsers },
        components: draftComps,
      };
      await unwrapStr(commands.pwExportSaveMap(org, project, map));
      await refresh();
    });

  // What the rows hold, as it would be saved, against what is saved. A row
  // blanked over a saved mapping is invalid: it blocks the save instead of
  // dropping the mapping. The text is trimmed, and so is a trailing slash;
  // it is never lowercased for the person (the save refuses what is not valid).
  const savedAreas = preview?.map.areas ?? {};
  const savedUsers = preview?.map.accounts?.[preview.environment] ?? {};
  const invalid = new Set<string>();
  const draftAreas: Record<string, Placement> = {};
  for (const [name, p] of Object.entries(areas ?? {})) {
    const typed = p.path.trim().replace(/[\s/]+$/, "");
    if (typed) draftAreas[name] = { path: typed };
    else if (p.path.trim() || name in savedAreas) invalid.add(name);
  }
  const draftUsers = Object.fromEntries(Object.entries(users ?? {}).filter(([, v]) => v));
  // Components the same way. The whole saved map is kept, so a mapping for a
  // component these cases do not use is not lost by saving.
  const savedComps = Object.fromEntries(Object.entries(preview?.map.components ?? {}).filter(([, v]) => v));
  const draftComps = Object.fromEntries(Object.entries(comps ?? {}).filter(([, v]) => v));
  // Own folders the same way: one blanked is invalid until it is given a
  // path or handed back to its area.
  const savedCases = preview?.map.cases ?? {};
  const invalidOwn = new Set<number>();
  const draftCases: Record<number, Placement> = {};
  for (const [id, raw] of Object.entries(own)) {
    const typed = raw.trim().replace(/[\s/]+$/, "");
    if (typed) draftCases[Number(id)] = { path: typed };
    else invalidOwn.add(Number(id));
  }
  const pathsOf = (m: Record<string, Placement | undefined>) =>
    JSON.stringify(Object.entries(m).map(([k, v]) => [k, v?.path ?? ""]).sort());
  const unsaved =
    invalid.size > 0 ||
    invalidOwn.size > 0 ||
    pathsOf(draftCases) !== pathsOf(savedCases) ||
    JSON.stringify(Object.entries(draftAreas).sort()) !== JSON.stringify(Object.entries(savedAreas).sort()) ||
    JSON.stringify(Object.entries(draftUsers).sort()) !== JSON.stringify(Object.entries(savedUsers).sort()) ||
    JSON.stringify(Object.entries(draftComps).sort()) !== JSON.stringify(Object.entries(savedComps).sort());

  const usedComponents = preview?.components ?? [];
  const commonComponents = preview?.common_components ?? [];
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
        commands.pwExportWrite(
          org,
          project,
          ticked,
          modules,
          replace,
          prefs.moduleRef ?? null,
          prefs.preconditionsRef ?? null,
          pbiId,
        ),
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

  // A typed path that is not one of the clone's features: Export creates it.
  // Only said once the clone's folders are read, and never blocks anything.
  const isNewFolder = (raw: string) => {
    const typed = raw.trim().replace(/[\s/]+$/, "");
    return featurePaths.length > 0 && typed !== "" && !featurePaths.includes(typed);
  };

  const place = (name: string): Placement => areas?.[name] ?? { path: "" };
  const setPlace = (name: string, patch: Partial<Placement>) =>
    setAreas((cur) => ({ ...(cur ?? {}), [name]: { ...place(name), ...patch } }));

  // A case with no Auto Run script has nothing to export: it is not listed.
  const listed = cases.filter((c) => c.has_script);
  const noScript = cases.length - listed.length;
  const inRepoCount = listed.filter((c) => c.in_repo && !c.excluded).length;
  const excludedCount = listed.filter((c) => c.excluded).length;
  const blockedCount = listed.filter((c) => !c.exportable && !c.excluded).length;
  // Grouped by each case's Module, in the order the cases come.
  const moduleOf = new Map(modules);
  const groups: [string, PreviewCase[]][] = [];
  for (const c of listed) {
    const name = moduleOf.get(c.case_id)?.trim() || "No module";
    const group = groups.find(([g]) => g === name);
    if (group) group[1].push(c);
    else groups.push([name, [c]]);
  }

  const tidy = (path: string) => path.trim().replace(/[\s/]+$/, "");
  /**
   * The folder the case's area holds now: the area's field as edited, not as
   * saved. The preview names a case's folder (`seg`, the saved one) but not
   * its area, so the area is the one(s) saved at that folder. Null when that
   * field is empty; the saved folder when the areas sharing it now differ.
   */
  const areaFolderOf = (c: PreviewCase): string | null => {
    if (!c.seg) return null;
    const saved = preview?.map.areas ?? {};
    const drafts = new Set(
      (preview?.areas ?? []).filter((n) => tidy(saved[n]?.path ?? "") === tidy(c.seg ?? "")).map((n) => tidy(areas?.[n]?.path ?? "")),
    );
    if (drafts.size !== 1) return c.seg;
    return [...drafts][0] || null;
  };
  /** The clone's folder a case's title points at, when it has a folder and no folder of its own. */
  const suggestionFor = (c: PreviewCase): string | null => {
    const area = c.excluded || !c.has_script || c.case_id in own ? null : areaFolderOf(c);
    return area ? suggestFolder(c.title, area, featurePaths) : null;
  };
  const suggestions = listed.flatMap((c) => {
    const path = suggestionFor(c);
    return path ? [[c.case_id, path] as const] : [];
  });
  const takeSuggestions = (picked: readonly (readonly [number, string])[]) =>
    setOwn((cur) => ({ ...cur, ...Object.fromEntries(picked) }));

  /** Where the case goes, and the control that gives it (or takes back) its own folder. */
  const folderLine = (c: PreviewCase) => {
    if (c.excluded) return null;
    const id = c.case_id;
    if (!(id in own)) {
      const suggested = suggestionFor(c);
      return (
        <div className="ml-[6.75rem] flex items-center gap-2">
          <span className="min-w-0 flex-1 truncate font-mono text-xs text-faint" title={c.seg ?? undefined}>
            {c.seg ?? "No folder yet"}
          </span>
          {suggested && (
            <>
              <span className="min-w-0 shrink truncate font-mono text-xs text-accent" title={suggested}>
                {`Suggested: ${suggested.split("/").pop()}`}
              </span>
              <Button
                size="sm"
                variant="ghost"
                aria-label={`Use the suggested folder for #${id}`}
                title="Send this case to the suggested folder instead of its area's"
                disabled={busy}
                onClick={() => takeSuggestions([[id, suggested]])}
              >
                <IconConfirm aria-hidden />
                Use
              </Button>
            </>
          )}
          <Button
            size="sm"
            variant="ghost"
            aria-label={`Give #${id} its own folder`}
            title="Send this case to a different folder from the rest of its area"
            disabled={busy}
            onClick={() => setOwn((cur) => ({ ...cur, [id]: c.seg ?? "" }))}
          >
            <IconEdit aria-hidden />
            Own folder
          </Button>
        </div>
      );
    }
    return (
      <div className="ml-[6.75rem] space-y-1">
        <div className="flex items-center gap-2">
          <Combobox
            className="min-w-0 flex-1"
            ariaLabel={`Folder for #${id}`}
            placeholder="sl/admin/performance/feature"
            allowCustom
            seedQuery
            options={featurePaths}
            triggerClassName="py-1.5 font-mono text-xs"
            value={own[id]}
            onChange={(v) => setOwn((cur) => ({ ...cur, [id]: v }))}
          />
          <Button
            size="sm"
            variant="ghost"
            aria-label={`Use the area's folder for #${id}`}
            title="Send this case where the rest of its area goes"
            disabled={busy}
            onClick={() =>
              setOwn((cur) => {
                const next = { ...cur };
                delete next[id];
                return next;
              })
            }
          >
            <IconUndo aria-hidden />
            Area's folder
          </Button>
        </div>
        {invalidOwn.has(id) && <p className="text-xs text-danger">needs a folder path</p>}
        {isNewFolder(own[id]) && <p className="text-xs text-warning">A new folder will be created.</p>}
      </div>
    );
  };

  const caseRow = (c: PreviewCase) => {
    const head = (
      <>
        <span className="w-16 shrink-0 pt-0.5 font-mono text-xs text-muted">{`#${c.case_id}`}</span>
        <span className="min-w-0 flex-1">
          <span className={`block text-sm ${c.exportable ? "text-text" : "text-muted"}`}>{c.title}</span>
          {c.in_repo && c.repo_note && !c.excluded && (
            <span className="block text-xs text-muted">{`${c.repo_note}${c.from_app ? " (exported by this app)" : ""}`}</span>
          )}
          {c.excluded && c.repo_note && <span className="block text-xs text-muted">{c.repo_note}</span>}
          {!c.exportable && c.reason && <span className="block text-xs text-muted">{c.reason}</span>}
          {/* Seen before anything is written; none of them stops Export. */}
          {c.warnings.map((w) => (
            <span key={w} className="flex items-start gap-1.5 text-xs text-warning">
              <IconWarning aria-hidden className="mt-0.5 size-3.5 shrink-0" />
              {w}
            </span>
          ))}
        </span>
        <span className="shrink-0 pt-0.5">
          {c.excluded ? (
            <StatusBadge tone="muted" title={repoStatus(c)}>
              Excluded
            </StatusBadge>
          ) : !c.exportable ? (
            <StatusBadge tone="danger" title={c.reason ?? undefined}>
              Cannot export
            </StatusBadge>
          ) : c.in_repo ? (
            <StatusBadge tone="warning" title={repoStatus(c)}>
              In the repo
            </StatusBadge>
          ) : (
            <StatusBadge tone="success">New</StatusBadge>
          )}
        </span>
      </>
    );
    if (c.exportable) {
      return (
        <div key={c.case_id} className="space-y-1 px-3 py-2 hover:bg-surface-2/60">
          <label className="flex cursor-pointer items-start gap-3">
            <span className="pt-0.5">
              <Checkbox checked={isTicked(c)} ariaLabel={`#${c.case_id} ${c.title}`} onCheckedChange={(on) => setTick(c, on)} />
            </span>
            {head}
          </label>
          {folderLine(c)}
        </div>
      );
    }
    return (
      <div key={c.case_id} className="space-y-1.5 px-3 py-2">
        <div className="flex items-start gap-3">
          <span className="pt-0.5">
            {/* The team left it out of the repo on purpose: never exported. */}
            {c.excluded ? (
              <Checkbox checked={false} disabled ariaLabel={`#${c.case_id} ${c.title}`} onCheckedChange={() => {}} />
            ) : (
              <span className="block size-4" aria-hidden />
            )}
          </span>
          {head}
        </div>
        {folderLine(c)}
        {c.add_user_command && (
          <div className="ml-[6.75rem] flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate rounded-md border border-border bg-bg px-2 py-1 font-mono text-xs text-text" title={c.add_user_command}>
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
    );
  };

  return (
    <Modal onClose={() => !busy && onClose()} labelledBy={titleId} className={`${MODAL_LARGE} flex flex-col overflow-hidden`}>
      <div className="space-y-3 border-b border-border px-5 pt-4 pb-3">
        <div>
          <h2 id={titleId} className="text-base font-semibold text-text">Export to Playwright</h2>
          <Hint>
            Copies the passing scripts of PBI {pbiId} into your clone of the Playwright repo. Nothing is run,
            committed or pushed.
          </Hint>
        </div>
        <div className="flex items-center gap-2">
          <div className="flex min-w-0 flex-1 items-center gap-2 rounded-md border border-border bg-bg px-2.5 py-1.5">
            <IconBrowse aria-hidden className="size-4 shrink-0 text-muted" />
            <span className="min-w-0 flex-1 truncate font-mono text-xs text-text" title={clone || undefined}>
              {clone || "No folder chosen yet."}
            </span>
          </div>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => void choose()}>
            <IconBrowse aria-hidden />
            Choose…
          </Button>
        </div>
        {preview && !preview.clone_ok && preview.clone_problem && <p className="text-xs text-danger">{preview.clone_problem}</p>}
      </div>

      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-5 py-4">
        {error && <p className="whitespace-pre-wrap rounded-md border border-danger/40 bg-danger/10 p-2 text-xs text-danger">{error}</p>}

        {result && (
          <section className="space-y-2 rounded-lg border border-success/40 bg-success/5 p-3" aria-label="Export summary">
            <h3 className="text-sm font-semibold text-success">Exported</h3>
            <ul className="space-y-0.5 font-mono text-xs text-text">
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
            <p className="text-xs font-medium text-text">Next:</p>
            <ol className="list-decimal space-y-0.5 pl-5 text-xs text-muted">
              {NEXT_STEPS.map((s) => (
                <li key={s}>{s}</li>
              ))}
            </ol>
            <Hint>
              Exporting a case again rewrites its section from Azure DevOps; the repo's Added by engineer, Also and Assertion floor blocks and every other section and file stay as they are.
            </Hint>
            {result.record_error && <p className="text-xs text-warning">{result.record_error}</p>}
            {refreshError && <p className="text-xs text-faint">{refreshError}</p>}
            <FinishInRepo clone={clone} org={org} project={project} result={result} />
          </section>
        )}

        {preview && areas && users && (
          <>
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-4" aria-label="Summary">
              <Stat value={ticked.length} label="to export" />
              <Stat value={inRepoCount} label="already in the repo" />
              <Stat value={excludedCount} label="excluded in the repo" />
              <Stat value={blockedCount} label="cannot export yet" />
            </div>

            <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_24rem] lg:items-start">
              <Card
                title={
                  <>
                    Cases <span className="text-xs font-normal text-faint">{listed.length}</span>
                  </>
                }
                aria-label="Cases"
                // The folder lists open past the card's edge.
                className="overflow-visible"
              >
                <div className="space-y-0.5 border-b border-border px-3 py-2">
                  <Hint>{SKIP_NOTE}</Hint>
                  <Hint>Each case goes to its area's folder. Own folder sends one case somewhere else.</Hint>
                  {suggestions.length > 1 && (
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={busy}
                      title="Send each of these cases to the folder suggested for it"
                      onClick={() => takeSuggestions(suggestions)}
                    >
                      <IconConfirm aria-hidden />
                      {`Use ${suggestions.length} suggested folders`}
                    </Button>
                  )}
                  {noScript > 0 && (
                    <Hint className="text-faint">{`${noScript} ${noScript === 1 ? "case without a script is" : "cases without a script are"} not shown.`}</Hint>
                  )}
                </div>
                {listed.length === 0 && <Hint className="px-3 py-4">None of these cases has an Auto Run script yet.</Hint>}
                {groups.map(([name, rows]) => (
                  <div key={name} role="group" aria-label={name}>
                    <div className="border-b border-border bg-surface-2 px-3 py-1 text-xs font-medium text-muted">
                      {name} <span className="text-faint">{rows.length}</span>
                    </div>
                    <div className="divide-y divide-border">{rows.map(caseRow)}</div>
                  </div>
                ))}
              </Card>

              <Card
                title="Mappings"
                actions={
                  <>
                    {unsaved && <span className="text-xs text-warning">Save the mappings first</span>}
                    <Button size="sm" variant="outline" disabled={busy || invalid.size > 0 || invalidOwn.size > 0} onClick={() => void saveMappings()}>
                      <IconConfirm aria-hidden />
                      Save mappings
                    </Button>
                  </>
                }
                bodyClassName="space-y-4 p-3"
                className="overflow-visible"
              >
                <section className="space-y-2">
                  <h4 className="text-xs font-medium text-text">Areas</h4>
                  <Hint>Where each area's specs go in the clone: the feature's folder under suites/, such as sl/admin/performance/rating-methods.</Hint>
                  {preview.clone_ok && (
                    <NewFolder clone={clone} paths={featurePaths} disabled={busy} onCreated={() => void structure.refetch()} />
                  )}
                  {preview.areas.length === 0 && <Hint>No areas yet.</Hint>}
                  {preview.areas.map((name) => (
                    <div key={name} className="space-y-1">
                      <div className="flex items-baseline justify-between gap-2">
                        <span className="text-sm text-text">{name}</span>
                        {invalid.has(name) && <span className="text-xs text-danger">needs a folder path</span>}
                      </div>
                      <Combobox
                        ariaLabel={`Folder for ${name}`}
                        placeholder="sl/admin/performance/feature"
                        allowCustom
                        seedQuery
                        options={featurePaths}
                        triggerClassName="py-1.5 font-mono text-xs"
                        value={place(name).path}
                        onChange={(v) => setPlace(name, { path: v })}
                      />
                      {isNewFolder(place(name).path) && <p className="text-xs text-warning">A new folder will be created.</p>}
                    </div>
                  ))}
                </section>

                <section className="space-y-2 border-t border-border pt-3">
                  <h4 className="text-xs font-medium text-text">Accounts</h4>
                  <Hint>The repo user each of your accounts signs in as.</Hint>
                  {preview.accounts.length === 0 && <Hint>No accounts yet.</Hint>}
                  {preview.accounts.map((key) => {
                    // A saved pick the clone no longer has: shown, and said, not left blank.
                    const stale = Boolean(users[key]) && !preview.user_keys.includes(users[key]);
                    return (
                      <div key={key} className="space-y-1">
                        <div className="grid grid-cols-[minmax(0,8rem)_minmax(0,1fr)] items-center gap-2">
                          <span className="truncate text-sm text-text" title={key}>
                            {key}
                          </span>
                          <Select
                            aria-label={`User for ${key}`}
                            triggerClassName="py-1.5 text-xs"
                            value={users[key] ?? ""}
                            onChange={(e) => setUsers((cur) => ({ ...(cur ?? {}), [key]: e.target.value }))}
                          >
                            <option value="">Not mapped</option>
                            {stale && <option value={users[key]}>{users[key]}</option>}
                            {preview.user_keys.map((k) => (
                              <option key={k} value={k}>
                                {k}
                              </option>
                            ))}
                          </Select>
                        </div>
                        {stale && <p className="text-xs text-warning">{`${users[key]} is not in this clone's users.json`}</p>}
                      </div>
                    );
                  })}
                </section>

                {usedComponents.length > 0 && (
                  <section className="space-y-2 border-t border-border pt-3">
                    <h4 className="text-xs font-medium text-text">Components</h4>
                    <Hint>The clone's common component each of your Auto Run components stands for. A mapped one is written as a Uses line in the case's section.</Hint>
                    {commonComponents.length === 0 && Object.keys(draftComps).length === 0 && (
                      <Hint>This clone has no common components yet.</Hint>
                    )}
                    {usedComponents
                      .filter((name) => commonComponents.length > 0 || draftComps[name])
                      .map((name) => {
                        const picked = comps?.[name] ?? "";
                        // A saved pick the clone no longer has: shown, and said, not left blank.
                        const stale = picked !== "" && !commonComponents.includes(picked);
                        return (
                          <div key={name} className="space-y-1">
                            <div className="grid grid-cols-[minmax(0,8rem)_minmax(0,1fr)] items-center gap-2">
                              <span className="truncate text-sm text-text" title={name}>
                                {name}
                              </span>
                              <Combobox
                                ariaLabel={`Common component for ${name}`}
                                triggerClassName="py-1.5 text-xs"
                                value={picked}
                                items={[
                                  { value: "", label: "Not mapped" },
                                  ...(stale ? [{ value: picked, label: picked }] : []),
                                  ...commonComponents.map((c) => ({ value: c, label: c })),
                                ]}
                                onChange={(v) => setComps((cur) => ({ ...(cur ?? {}), [name]: v }))}
                              />
                            </div>
                            {stale && <p className="text-xs text-warning">{`${picked} is not under suites/common-components in this clone`}</p>}
                          </div>
                        );
                      })}
                  </section>
                )}
              </Card>
            </div>
          </>
        )}
      </div>

      <div className="flex items-center justify-end gap-2 border-t border-border bg-surface px-5 py-3">
        {preview && !result && unsaved ? (
          <Hint className="mr-auto text-warning">Save the mappings to export.</Hint>
        ) : (
          preview && !result && ticked.length === 0 && listed.length > 0 && <Hint className="mr-auto">Tick at least one case to export.</Hint>
        )}
        <Button variant="ghost" size="sm" disabled={busy} onClick={onClose}>
          <IconCancel aria-hidden />
          {result ? "Close" : "Cancel"}
        </Button>
        <Button size="sm" disabled={!canExport} onClick={() => void doExport()}>
          <IconExport aria-hidden />
          {`Export (${ticked.length})`}
        </Button>
      </div>
    </Modal>
  );
}

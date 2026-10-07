// Export to Playwright: copies this PBI's passing Auto Run scripts into a
// local clone of the team's PHR-PLAYWRIGHT-AUTOMATION repo, as raw specs plus
// a section in the area's test-case file. TCM only writes files; running,
// refactoring and committing them is the person's (or Claude Code's) job in
// the clone, which the summary says in order.
//
// Four parts, top to bottom: the clone folder, where each area's specs live,
// which repo user each of TCM's accounts signs in as, and the cases. The
// preview (what each case needs, and why one cannot go) is read again after
// a mapping is saved or the clone changes, so the reasons stay current.

import { useCallback, useEffect, useId, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  commands,
  type ExportMap,
  type ExportResult,
  type Placement,
  type Preview,
} from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Input } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { MODAL_LARGE } from "./modalWidths";
import { IconBrowse, IconCancel, IconConfirm, IconCopy, IconExport } from "../../lib/actionIcons";
import { copyText } from "../../lib/clipboard";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { unwrapStr } from "../../lib/ipc";

const SIDES = ["admin", "self"];
const sectionTitle = "text-xs font-semibold uppercase tracking-wide text-muted";
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

const NEXT_STEPS = [
  "Capture navigation for any area listed above as missing.",
  "Run each raw spec in the generated project, with the seed set to the case's user key.",
  "Ask Claude Code to run test-refactorer on the raw specs.",
  "Run npm run lint:tests -- --require-specs",
  "Review the changes and commit them.",
];

export default function PlaywrightExportDialog({
  org,
  project,
  pbiId,
  caseIds,
  onClose,
}: {
  org: string;
  project: string;
  pbiId: number;
  caseIds: number[];
  onClose: () => void;
}) {
  const titleId = useId();
  const { prefs } = useFieldRefs(org, project);
  const [clone, setClone] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  /** The area rows and account picks being edited; seeded once from the saved map. */
  const [areas, setAreas] = useState<Record<string, Placement> | null>(null);
  const [users, setUsers] = useState<Record<string, string> | null>(null);
  /** Exportable cases the person unticked; everything else exportable is ticked. */
  const [unticked, setUnticked] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<ExportResult | null>(null);

  const refresh = useCallback(async () => {
    const p = await unwrapStr(commands.pwExportPreview(org, project, caseIds));
    setPreview(p);
    setAreas((cur) => cur ?? { ...(p.map.areas ?? {}) });
    setUsers((cur) => cur ?? { ...(p.map.accounts?.[p.environment] ?? {}) });
  }, [org, project, caseIds]);

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
      if (!preview || !areas || !users) return;
      const placed = Object.fromEntries(
        Object.entries(areas).filter(([, p]) => p.module.trim() && p.feature.trim()),
      );
      const picked = Object.fromEntries(Object.entries(users).filter(([, v]) => v));
      const map: ExportMap = {
        areas: placed,
        accounts: { ...(preview.map.accounts ?? {}), [preview.environment]: picked },
      };
      await unwrapStr(commands.pwExportSaveMap(org, project, map));
      await refresh();
    });

  const cases = preview?.cases ?? [];
  const ticked = cases.filter((c) => c.exportable && !unticked.has(c.case_id)).map((c) => c.case_id);
  const canExport = Boolean(preview?.clone_ok) && ticked.length > 0 && !busy;

  const doExport = () =>
    attempt(async () => {
      setResult(
        await unwrapStr(
          commands.pwExportWrite(org, project, ticked, prefs.moduleRef ?? null, prefs.preconditionsRef ?? null),
        ),
      );
      await refresh();
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
                </div>
              ))}
            </section>

            <section className="space-y-2">
              <h3 className={sectionTitle}>Accounts</h3>
              {preview.accounts.length === 0 && <p className="text-xs text-muted">No accounts yet.</p>}
              {preview.accounts.map((key) => (
                <div key={key} className="flex flex-wrap items-center gap-2">
                  <span className="w-48 text-sm text-text">{key}</span>
                  <Select
                    aria-label={`User for ${key}`}
                    className="w-64"
                    value={users[key] ?? ""}
                    onChange={(e) => setUsers((cur) => ({ ...(cur ?? {}), [key]: e.target.value }))}
                  >
                    <option value="">Not mapped</option>
                    {preview.user_keys.map((k) => (
                      <option key={k} value={k}>{k}</option>
                    ))}
                  </Select>
                </div>
              ))}
              <div className="flex justify-end">
                <Button size="sm" variant="outline" disabled={busy} onClick={() => void saveMappings()}>
                  <IconConfirm aria-hidden />
                  Save mappings
                </Button>
              </div>
            </section>

            <section className="space-y-2">
              <h3 className={sectionTitle}>Cases</h3>
              {cases.map((c) =>
                c.exportable ? (
                  <label key={c.case_id} className="flex items-center gap-2 text-sm text-text">
                    <Checkbox
                      checked={!unticked.has(c.case_id)}
                      ariaLabel={`#${c.case_id} ${c.title}`}
                      onCheckedChange={(on) =>
                        setUnticked((cur) => {
                          const next = new Set(cur);
                          if (on) next.delete(c.case_id);
                          else next.add(c.case_id);
                          return next;
                        })
                      }
                    />
                    <span>{`#${c.case_id} ${c.title}`}</span>
                  </label>
                ) : (
                  <div key={c.case_id} className="space-y-1 text-sm text-faint">
                    <p>{`#${c.case_id} ${c.title}`}</p>
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
              {result.cases.map(([id, file]) => (
                <li key={id}>{`#${id} → ${file}`}</li>
              ))}
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
              Exporting a case again replaces that case's whole section in its test-case file.
            </p>
          </section>
        )}
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

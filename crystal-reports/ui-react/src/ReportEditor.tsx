/**
 * ReportEditor — composition root: tree + layout preview + property inspector,
 * wired to the Tauri commands via api.ts. Owns the IR state; every applied batch
 * replaces it with the backend's refreshed IR (the single source of truth).
 */
import { useCallback, useState } from "react";
import type { EditOp, Ir, ObjectIr } from "./types";
import * as api from "./api";
import { ReportTree } from "./ReportTree";
import { PropertyInspector, type Selection } from "./PropertyInspector";
import { LayoutPreview } from "./LayoutPreview";

export function ReportEditor() {
  const [ir, setIr] = useState<Ir | null>(null);
  const [selection, setSelection] = useState<Selection>({ object: null, section: null });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);

  const open = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    try {
      setIr(await api.openReport(path));
      setSelection({ object: null, section: null });
      setDirty(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const apply = useCallback(
    async (ops: EditOp[]) => {
      if (!ir) return;
      setBusy(true);
      setError(null);
      try {
        setIr(await api.applyEdits(ops));
        setDirty(true);
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [ir],
  );

  const save = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await api.saveReport();
      setDirty(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const selectObject = (o: ObjectIr) => setSelection({ object: o, section: null });

  if (!ir) {
    return (
      <div className="flex h-full items-center justify-center bg-slate-900 text-slate-300">
        <button
          className="rounded bg-sky-600 px-4 py-2 font-medium text-white hover:bg-sky-500"
          // wire to @tauri-apps/plugin-dialog open({ filters: [{ name: "Crystal Reports", extensions: ["rpt"] }] })
          onClick={() => open("C:/reports/Sample.rpt")}
        >
          Open report…
        </button>
        {error && <p className="mt-3 text-sm text-red-400">{error}</p>}
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col bg-slate-900 text-slate-100">
      <header className="flex items-center gap-3 border-b border-slate-700 px-4 py-2">
        <span className="truncate font-medium">{ir.file}</span>
        {dirty && <span className="text-xs text-amber-400">● unsaved</span>}
        {ir.warnings.length > 0 && (
          <span className="text-xs text-amber-400" title={ir.warnings.join("\n")}>
            {ir.warnings.length} warning(s)
          </span>
        )}
        <button
          className="ml-auto rounded border border-slate-600 px-3 py-1 text-sm hover:bg-slate-700/40"
          disabled={busy}
          onClick={() => open(ir.file)}
        >
          Revert
        </button>
        <button
          className="rounded bg-sky-600 px-3 py-1 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-50"
          disabled={busy || !dirty}
          onClick={save}
        >
          Save
        </button>
      </header>

      <div className="flex min-h-0 flex-1">
        <aside className="w-64 shrink-0 border-r border-slate-700">
          <ReportTree
            ir={ir}
            selectedId={selection.object?.id ?? null}
            onSelectObject={selectObject}
            onSelectSection={(area, section) =>
              setSelection({ object: null, section: { area, section } })
            }
          />
        </aside>
        <main className="min-w-0 flex-1">
          <LayoutPreview ir={ir} selectedId={selection.object?.id ?? null} onSelectObject={selectObject} />
        </main>
        <aside className="w-80 shrink-0 border-l border-slate-700">
          <PropertyInspector ir={ir} selection={selection} onApply={apply} busy={busy} error={error} />
        </aside>
      </div>
    </div>
  );
}

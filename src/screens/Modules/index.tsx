import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconBack } from "../../lib/actionIcons";
import { envKeys, loadEnvironments } from "../../lib/environments";
import { turnOnNetworkToolsWhereUnset } from "../../lib/networkTools";
import type { Recommended } from "../../bindings";
import { realModulesSnapshot, setModule, useModules, useModulesHydrated, type Module } from "../../lib/modules";
import { noteModuleSwitch } from "../../lib/newNav";
import ModuleTree from "./ModuleTree";
import { MODULE_INFO, infoOf, recommendedNote, switchOffWarning } from "./modulesCopy";

/** Every module on that hangs, directly or not, below `m`, in tree order. */
function below(m: Module, on: ReadonlySet<Module>): Module[] {
  const gone = new Set<Module>([m]);
  const out: Module[] = [];
  for (const { module, parent } of MODULE_INFO) {
    if (parent && gone.has(parent)) {
      gone.add(module);
      if (on.has(module)) out.push(module);
    }
  }
  return out;
}

/** The Modules page: a tree of what can be switched on, opened from Settings. */
export default function Modules({ onBack }: { onBack: () => void }) {
  const modules = useModules();
  const hydrated = useModulesHydrated();
  const [busy, setBusy] = useState<Module | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const qc = useQueryClient();
  const [confirm, setConfirm] = useState<{ module: Module; also: Module[] } | null>(null);

  const apply = async (m: Module, next: boolean) => {
    setError(null);
    setNote(null);
    setBusy(m);
    try {
      const before = realModulesSnapshot();
      const recommended = await setModule(m, next);
      // A person's own switch: mark in the sidebar what it revealed, or
      // take the mark away from what it hid.
      noteModuleSwitch(before, realModulesSnapshot());
      if (recommended) await applyRecommended(recommended);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  /** The first switch-on of Auto Test: Rust turned its own recommended items
   * on; the Network tools are the webview's, per environment, so they are
   * applied here. The settings (mode, video) changed in Rust, so re-read them. */
  const applyRecommended = async (r: Recommended) => {
    let networkTools = false;
    if (r.network_tools) {
      try {
        const view = await qc.fetchQuery({ queryKey: envKeys.list, queryFn: loadEnvironments });
        networkTools = turnOnNetworkToolsWhereUnset(view.environments.map((e) => e.id)) > 0;
      } catch {
        // The environments could not be read: the module is on either way,
        // and the note simply does not claim the Network tools.
      }
    }
    void qc.invalidateQueries({ queryKey: ["app-settings"] });
    setNote(recommendedNote({ api_templates: r.api_templates, network_tools: networkTools, video: r.video }));
  };

  const toggle = (m: Module, next: boolean) => {
    if (!next) {
      const also = below(m, modules);
      if (also.length > 0) {
        setConfirm({ module: m, also });
        return;
      }
    }
    void apply(m, next);
  };

  return (
    // Fills the area it is given: the header and notes keep their height and
    // only the board (which scales to fit) scrolls, and only when it must.
    // The gap at the bottom keeps its frame off the window edge.
    <div className="flex min-h-0 flex-1 flex-col gap-4 pb-6">
      <div className="flex shrink-0 flex-wrap items-center gap-3">
        <Button size="sm" variant="outline" onClick={onBack}>
          <IconBack aria-hidden />
          Back to Settings
        </Button>
        <h2 className="text-base font-semibold text-text">Modules</h2>
      </div>
      <p className="max-w-4xl shrink-0 text-sm text-muted">
        Switch on the parts of the app you need. A module needs the one above it, and switching one off switches off
        those below it. Nothing is deleted.
      </p>
      {error && (
        <p role="alert" className="shrink-0 rounded-md border border-danger px-3 py-2 text-sm text-danger">
          {error}
        </p>
      )}
      {note && (
        <p role="status" className="shrink-0 rounded-md border border-border px-3 py-2 text-sm text-muted">
          {note}
        </p>
      )}
      <ModuleTree modules={modules} hydrated={hydrated} busy={busy} onToggle={toggle} />
      {confirm && (
        <Modal onClose={() => setConfirm(null)} className="w-full max-w-sm space-y-4 p-5" label="Switch off module">
          <p className="text-sm text-text">
            {switchOffWarning(
              infoOf(confirm.module).name,
              confirm.also.map((m) => infoOf(m).name),
            )}
          </p>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="outline" onClick={() => setConfirm(null)}>
              Cancel
            </Button>
            <Button
              size="sm"
              onClick={() => {
                const m = confirm.module;
                setConfirm(null);
                void apply(m, false);
              }}
            >
              Switch off
            </Button>
          </div>
        </Modal>
      )}
    </div>
  );
}

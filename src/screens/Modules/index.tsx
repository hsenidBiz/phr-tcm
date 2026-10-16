import { useState } from "react";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconBack } from "../../lib/actionIcons";
import { setModule, useModules, useModulesHydrated, type Module } from "../../lib/modules";
import ModuleTree from "./ModuleTree";
import { MODULE_INFO, infoOf, switchOffWarning } from "./modulesCopy";

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
  const [confirm, setConfirm] = useState<{ module: Module; also: Module[] } | null>(null);

  const apply = async (m: Module, next: boolean) => {
    setError(null);
    setBusy(m);
    try {
      await setModule(m, next);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
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
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <Button size="sm" variant="outline" onClick={onBack}>
          <IconBack aria-hidden />
          Back to Settings
        </Button>
        <h2 className="text-base font-semibold text-text">Modules</h2>
      </div>
      <p className="max-w-4xl text-sm text-muted">
        Switch on the parts of the app you need. A module needs the one above it, and switching one off switches off
        those below it. Nothing is deleted.
      </p>
      {error && (
        <p role="alert" className="rounded-md border border-danger px-3 py-2 text-sm text-danger">
          {error}
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

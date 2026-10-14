import { useMutation } from "@tanstack/react-query";
import { useId } from "react";
import { commands, type TemplatesImportResult } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconConfirm, IconImport } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";

/** The name of the file a person picked - never the folder it is in. */
function fileNameOf(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

/** One titled list of the result, left out when it is empty. */
function ResultList({ title, lines, tone }: { title: string; lines: string[]; tone?: string }) {
  if (lines.length === 0) return null;
  return (
    <section aria-label={title} className="space-y-1">
      <h3 className={tone ?? "text-xs font-semibold text-text"}>
        {title} ({lines.length})
      </h3>
      <ul className="space-y-0.5 pl-3 text-xs text-muted">
        {lines.map((line, i) => (
          <li key={i}>{line}</li>
        ))}
      </ul>
    </section>
  );
}

function Result({ result }: { result: TemplatesImportResult }) {
  const imported = result.added.length + result.replaced.length;
  return (
    <div className="min-h-0 space-y-3 overflow-y-auto">
      {imported === 0 ? (
        <p className="text-xs text-muted">Nothing was imported.</p>
      ) : (
        <p className="text-xs text-muted">
          Everything imported arrives unproven - prove it on your site before relying on it.
        </p>
      )}
      <ResultList title="Added" lines={result.added} />
      <ResultList title="Replaced" lines={result.replaced} />
      <ResultList
        title="Cannot run yet"
        tone="text-xs font-semibold text-warning"
        lines={result.notes.map((n) => `${n.title}: ${n.note}`)}
      />
      <ResultList
        title="Skipped"
        tone="text-xs font-semibold text-danger"
        lines={result.skipped.map((s) => `${s.id}: ${s.reason}`)}
      />
    </div>
  );
}

/**
 * Importing a file of API templates and flows: a warning first - a same-id
 * one is replaced and arrives unproven - then what was added, replaced and
 * skipped. `onImported` runs as soon as the files are written, so the tab
 * reads its lists again behind the result.
 */
export default function ImportTemplates({
  org,
  project,
  path,
  onClose,
  onImported,
}: {
  org: string;
  project: string;
  /** The file the person picked. */
  path: string;
  onClose: () => void;
  onImported: () => void;
}) {
  const headingId = useId();
  const name = fileNameOf(path);
  const run = useMutation({
    mutationFn: () => unwrapStr(commands.apiTemplatesImport(org, project, path)),
    onSuccess: () => onImported(),
    onError: (e) => {
      toast.error(e.message);
      onClose();
    },
  });

  return (
    <Modal onClose={onClose} labelledBy={headingId} className="flex max-h-[80vh] w-full max-w-lg flex-col gap-3 p-5">
      <h2 id={headingId} className="text-sm font-semibold text-text">
        {run.data ? `Imported ${name}` : `Import ${name}?`}
      </h2>
      {run.data ? (
        <>
          <Result result={run.data} />
          <div className="flex justify-end">
            <Button size="sm" variant="outline" onClick={onClose}>
              <IconConfirm aria-hidden />
              Done
            </Button>
          </div>
        </>
      ) : (
        <>
          <p className="text-xs text-muted">
            Templates and flows with the same id as ones saved here are replaced, and arrive unproven - prove them
            on your site before relying on them.
          </p>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" disabled={run.isPending} onClick={onClose}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" disabled={run.isPending} onClick={() => run.mutate()}>
              <IconImport aria-hidden />
              Import
            </Button>
          </div>
        </>
      )}
    </Modal>
  );
}

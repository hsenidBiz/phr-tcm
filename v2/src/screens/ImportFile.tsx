import { useMutation } from "@tanstack/react-query";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type PbiHit } from "../bindings";
import QueueSection from "../components/QueueSection";
import { Button } from "../components/ui/button";
import { useQueue } from "../hooks/useQueue";

export default function ImportFile({
  org,
  project,
  pbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
}) {
  const { queue, setQueue } = useQueue(org, pbi?.id ?? null);
  const [warnings, setWarnings] = useState<string[]>([]);

  const importFile = useMutation({
    mutationFn: async () => {
      const path = await open({
        multiple: false,
        filters: [{ name: "Import", extensions: ["xlsx", "csv", "json"] }],
      });
      if (typeof path !== "string") return null;
      const r = await commands.parseImportFile(path);
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: (data) => {
      if (!data) return;
      setQueue((q) => [...q, ...data.cases]);
      setWarnings(data.warnings);
      toast.success(
        `Imported ${data.cases.length} case${data.cases.length === 1 ? "" : "s"}` +
          (data.warnings.length ? ` with ${data.warnings.length} warning(s)` : ""),
      );
    },
    onError: (e) => toast.error(`Import failed: ${e.message}`),
  });

  const saveTemplate = useMutation({
    mutationFn: async () => {
      const path = await save({
        defaultPath: "test-case-template.xlsx",
        filters: [{ name: "Excel", extensions: ["xlsx"] }],
      });
      if (!path) return;
      const r = await commands.writeTemplate(path);
      if (r.status === "error") throw new Error(r.error);
      toast.success("Template saved.");
    },
    onError: (e) => toast.error(`Could not save template: ${e.message}`),
  });

  if (!org || !project || !pbi) {
    return (
      <p className="text-sm text-muted">
        Pick an organization, project and PBI in the bar above, then import a
        spreadsheet or JSON file.
      </p>
    );
  }

  return (
    <div className="space-y-4">
      <section className="max-w-2xl space-y-3 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">Import test cases</h2>
        <p className="text-sm text-muted">
          9-column xlsx/csv or AI round-trip JSON. A filled TestCaseID updates
          that work item; a blank one creates a new case.
        </p>
        <div className="flex gap-2">
          <Button disabled={importFile.isPending} onClick={() => importFile.mutate()}>
            {importFile.isPending ? "Importing..." : "Import file..."}
          </Button>
          <Button variant="outline" onClick={() => saveTemplate.mutate()}>
            Save template...
          </Button>
        </div>
        {warnings.length > 0 && (
          <ul className="max-h-32 space-y-0.5 overflow-y-auto text-xs text-warning">
            {warnings.map((w, i) => (
              <li key={i}>{w}</li>
            ))}
          </ul>
        )}
      </section>

      <QueueSection org={org} project={project} pbiId={pbi.id} queue={queue} setQueue={setQueue} />
    </div>
  );
}

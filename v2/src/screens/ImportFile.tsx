import { useMutation } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type PbiHit } from "../bindings";
import AiGuideWizard from "../components/AiGuideWizard";
import PickPbiEmpty from "../components/PickPbiEmpty";
import QueueSection from "../components/QueueSection";
import { Button } from "../components/ui/button";
import { useQueue } from "../hooks/useQueue";

export default function ImportFile({
  org,
  project,
  pbi,
  onPickPbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
  onPickPbi?: (pbi: PbiHit) => void;
}) {
  const { queue, setQueue } = useQueue(org, pbi?.id ?? null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [guideOpen, setGuideOpen] = useState(false);

  const importFile = useMutation({
    mutationFn: async () => {
      // JSON is the import format (the AI round-trip file exports produce).
      const path = await open({
        multiple: false,
        filters: [{ name: "Test cases (JSON)", extensions: ["json"] }],
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

  if (!org || !project || !pbi) {
    return (
      <PickPbiEmpty
        message="Pick an organization, project and PBI in the bar above, then import a JSON file of test cases."
        org={org}
        project={project}
        onPickPbi={onPickPbi}
      />
    );
  }

  return (
    <div className="space-y-4">
      <section className="space-y-3 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">Import test cases</h2>
        <p className="text-sm text-muted">
          The JSON round-trip format (what Export JSON produces, AI-editable).
          A kept "id" updates that work item; a null id creates a new case.
        </p>
        <div className="flex gap-2">
          <Button disabled={importFile.isPending} onClick={() => importFile.mutate()}>
            {importFile.isPending ? "Importing" : "Import JSON"}
          </Button>
          <Button variant="outline" onClick={() => setGuideOpen(true)}>
            Generate AI guide…
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

      {guideOpen && (
        <AiGuideWizard org={org} project={project} area={null} onClose={() => setGuideOpen(false)} />
      )}
    </div>
  );
}

import { useMutation } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { commands, type PbiHit, type SharedQueue } from "../bindings";
import PickPbiEmpty from "../components/PickPbiEmpty";
import QueueSection from "../components/QueueSection";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Modal } from "../components/ui/modal";
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
  const [shareLink, setShareLink] = useState("");
  // A fetched draft whose PBI differs from the current selection: the
  // queue is stored PER PBI, so loading it here would hide the cases the
  // moment the user switches. Ask first.
  const [choice, setChoice] = useState<SharedQueue | null>(null);
  // Cases waiting for a specific PBI to become current. Switching PBI is
  // async (App owns it, useQueue re-keys on the new id), so the load is
  // deferred until `pbi` actually matches - writing immediately would put
  // them in the OLD PBI's queue, which is the bug this fixes.
  const [pendingFor, setPendingFor] = useState<{
    pbiId: number;
    data: SharedQueue;
    extraWarnings: string[];
  } | null>(null);

  useEffect(() => {
    if (!pendingFor || pbi?.id !== pendingFor.pbiId) return;
    const { data, extraWarnings } = pendingFor;
    setQueue((q) => [...q, ...data.cases]);
    setWarnings([...extraWarnings, ...data.warnings]);
    setPendingFor(null);
    toast.success(
      `Imported ${data.cases.length} shared case${data.cases.length === 1 ? "" : "s"} for review.`,
    );
  }, [pendingFor, pbi?.id, setQueue]);

  // A pasted share link (see ado_share.rs): one-time use - a successful
  // import revokes it, so a second paste tells the user it's spent.
  const importShared = useMutation({
    mutationFn: async () => {
      const r = await commands.fetchSharedQueue(shareLink.trim());
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: (data) => {
      setShareLink("");
      if (pbi && data.pbi_id !== pbi.id) {
        setChoice(data); // ask which PBI's queue to load into
        return;
      }
      setPendingFor({ pbiId: data.pbi_id, data, extraWarnings: [] });
    },
    onError: (e) => toast.error(`Shared import failed: ${e.message}`),
  });

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
        </div>
        <div className="space-y-1 border-t border-border/60 pt-3">
          <p className="text-xs text-muted">
            Or paste a share link a teammate sent you (one-time use - importing it revokes the link):
          </p>
          <div className="flex gap-2">
            <Input
              aria-label="Share link"
              placeholder="tcm-share:…"
              className="id-mono flex-1 py-1.5 text-xs"
              value={shareLink}
              onChange={(e) => setShareLink(e.target.value)}
            />
            <Button
              variant="outline"
              disabled={!shareLink.trim() || importShared.isPending}
              onClick={() => importShared.mutate()}
            >
              {importShared.isPending ? "Fetching" : "Import shared"}
            </Button>
          </div>
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

      {choice && (
        <Modal onClose={() => setChoice(null)} className="w-full max-w-md p-4">
          <h2 className="text-sm font-semibold text-text">This draft is for a different PBI</h2>
          <p className="mt-2 text-sm text-muted">
            It was shared for{" "}
            <span className="text-text">
              #{choice.pbi_id}
              {choice.pbi_title ? ` ${choice.pbi_title}` : ""}
            </span>
            , but you have <span className="text-text">#{pbi.id} {pbi.title}</span> selected.
          </p>
          <p className="mt-2 text-xs text-faint">
            Queued cases are kept per PBI, so they will appear under whichever you choose.
          </p>
          <div className="mt-4 flex flex-wrap justify-end gap-2">
            <Button variant="outline" size="sm" onClick={() => setChoice(null)}>
              Cancel
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                setPendingFor({
                  pbiId: pbi.id,
                  data: choice,
                  extraWarnings: [
                    `This draft was shared for PBI #${choice.pbi_id}, but you loaded it under #${pbi.id} - check before creating.`,
                  ],
                });
                setChoice(null);
              }}
            >
              Stay on #{pbi.id}
            </Button>
            <Button
              size="sm"
              disabled={!onPickPbi}
              title={onPickPbi ? undefined : "Switching is unavailable here"}
              onClick={() => {
                setPendingFor({ pbiId: choice.pbi_id, data: choice, extraWarnings: [] });
                onPickPbi?.({
                  id: choice.pbi_id,
                  title: choice.pbi_title,
                  work_item_type: choice.pbi_work_item_type,
                });
                setChoice(null);
              }}
            >
              Switch to #{choice.pbi_id}
            </Button>
          </div>
        </Modal>
      )}
    </div>
  );
}

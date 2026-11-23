import { useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { commands, type DbDatabase } from "../bindings";
import { IconAdd, IconCancel, IconEdit, IconRefresh, IconRemove } from "../lib/actionIcons";
import { forgetRemovedDb } from "../lib/dbServer";
import {
  checkedAtTitle,
  recheckDb,
  statusOf,
  useDbCheckProgress,
  useDbWriteVerdicts,
  verdictOf,
} from "../lib/dbWriteCheck";
import { toast } from "../lib/toast";
import { DbCredentialsModal } from "./DbCredentialsModal";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Modal } from "./ui/modal";

/**
 * Every saved database in one place: filter them, add one of your own, edit
 * a login, delete one of your own, and ask the server again what a login may
 * do. The AI Bridge card keeps only the picker and the write switch.
 *
 * A shipped database is the app's, so only its login is the person's to
 * change - it has no Delete. Its password never reaches this window: the
 * list is the public view Rust sends.
 */
export function ManageDatabasesDialog({ databases, onClose }: { databases: DbDatabase[]; onClose: () => void }) {
  const qc = useQueryClient();
  const titleId = useId();
  const [filter, setFilter] = useState("");
  const [adding, setAdding] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const editing = databases.find((d) => d.id === editingId) ?? null;
  const [deleting, setDeleting] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<{ id: string; text: string } | null>(null);
  const verdicts = useDbWriteVerdicts(true);
  const progress = useDbCheckProgress();

  const q = filter.trim().toLowerCase();
  const shown = q
    ? databases.filter((d) => d.label.toLowerCase().includes(q) || d.server.toLowerCase().includes(q))
    : databases;

  // A saved or edited login is asked about again at once: a different login
  // may be allowed to do something the old one was not.
  const saved = (d: DbDatabase) => {
    void qc.invalidateQueries({ queryKey: ["db-databases"] });
    if (d.server && d.user && d.has_password) void recheckDb(qc, d.id);
  };

  const remove = async (d: DbDatabase) => {
    setBusy(true);
    setProblem(null);
    try {
      const res = await commands.dbRemoveCustom(d.id);
      setDeleting(null);
      if (res.status === "error") {
        // In use by an environment, most likely: the sentence names it.
        setProblem({ id: d.id, text: res.error });
        return;
      }
      forgetRemovedDb(d.id);
      toast.success(`Removed ${d.label}.`);
      await qc.invalidateQueries({ queryKey: ["db-databases"] });
    } catch (e) {
      setProblem({ id: d.id, text: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
    <Modal onClose={onClose} labelledBy={titleId} className="flex max-h-[80vh] w-[560px] max-w-full flex-col gap-3 p-4">
      <h2 id={titleId} className="text-sm font-semibold text-text">
        Manage databases
      </h2>
      <div className="flex items-center gap-2">
        <Input
          aria-label="Filter databases"
          className="min-w-0 flex-1 py-1.5 text-xs"
          placeholder="Filter by name or server"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <Button size="sm" variant="outline" onClick={() => setAdding(true)}>
          <IconAdd aria-hidden />
          Add database
        </Button>
      </div>

      <ul className="min-h-0 flex-1 space-y-1 overflow-y-auto">
        {shown.map((d) => {
          const check = verdictOf(verdicts.data, d.id);
          const checking = progress.checking.has(d.id);
          const hasLogin = Boolean(d.server && d.user && d.has_password);
          return (
            <li key={d.id} className="space-y-1 rounded-md border border-border/60 p-2">
              <div className="flex items-center gap-2">
                <div className="min-w-0 flex-1">
                  <p className="truncate text-xs font-medium text-text">{d.label}</p>
                  <p className="truncate text-[11px] text-muted">
                    {d.server ? `${d.database} · ${d.server}` : "Not set up yet"}
                  </p>
                </div>
                <span className="shrink-0 text-[11px] text-faint" title={checkedAtTitle(check)}>
                  {checking ? "Checking…" : progress.failed.has(d.id) ? "Check failed" : statusOf(check)}
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  aria-label={`Recheck ${d.label}`}
                  disabled={!hasLogin || checking}
                  onClick={() => void recheckDb(qc, d.id)}
                >
                  <IconRefresh aria-hidden />
                </Button>
                <Button size="sm" variant="ghost" aria-label={`Edit ${d.label}`} onClick={() => setEditingId(d.id)}>
                  <IconEdit aria-hidden />
                </Button>
                {!d.shipped && (
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={`Delete ${d.label}`}
                    onClick={() => {
                      setProblem(null);
                      setDeleting(d.id);
                    }}
                  >
                    <IconRemove aria-hidden />
                  </Button>
                )}
              </div>
              {deleting === d.id && (
                <div className="flex flex-wrap items-center gap-2 border-t border-border/60 pt-2">
                  <span className="min-w-0 flex-1 text-xs text-text">
                    Remove {d.label}? Its saved login is deleted from this machine.
                  </span>
                  <Button size="sm" variant="ghost" onClick={() => setDeleting(null)}>
                    <IconCancel aria-hidden />
                    Keep
                  </Button>
                  <Button size="sm" variant="outline" disabled={busy} onClick={() => void remove(d)}>
                    <IconRemove aria-hidden />
                    Remove
                  </Button>
                </div>
              )}
              {problem?.id === d.id && (
                <p role="status" className="break-words text-xs text-danger">
                  {problem.text}
                </p>
              )}
            </li>
          );
        })}
        {shown.length === 0 && (
          <li className="px-1 py-2 text-xs text-muted">
            {databases.length === 0 ? "No databases saved yet." : "No database matches that filter."}
          </li>
        )}
      </ul>

      <div className="flex justify-end">
        <Button size="sm" variant="outline" onClick={onClose}>
          <IconCancel aria-hidden />
          Close
        </Button>
      </div>
    </Modal>
    {/* Beside the dialog, not inside it: each is a dialog of its own. */}
    {adding && <DbCredentialsModal database={null} onClose={() => setAdding(false)} onSaved={saved} />}
    {editing && <DbCredentialsModal database={editing} onClose={() => setEditingId(null)} onSaved={saved} />}
    </>
  );
}

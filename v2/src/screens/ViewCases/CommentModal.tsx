import { MessageSquare, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { TestCaseFull } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Textarea } from "../../components/ui/input";

/** A small dialog for reading (and editing) one case's local comment,
 * opened from the row's Comment chip - no need to expand the whole case. */
export default function CommentModal({
  c,
  note,
  onSave,
  onClose,
}: {
  c: TestCaseFull;
  note: string;
  onSave: (text: string) => void;
  onClose: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(note);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={`Comment for #${c.id}`}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
    >
      <div className="modal-in w-full max-w-md rounded-lg border border-border bg-surface shadow-2xl">
        <div className="flex items-center gap-2 border-b border-border px-4 py-3">
          <MessageSquare size={14} className="shrink-0 text-accent" />
          <span className="truncate text-sm font-semibold text-text">
            <span className="id-mono text-faint">#{c.id}</span> {c.title}
          </span>
          <button
            aria-label="Close comment"
            className="ml-auto rounded p-1 text-muted hover:text-text"
            onClick={onClose}
          >
            <X size={15} />
          </button>
        </div>

        <div className="space-y-2 p-4">
          {editing ? (
            <>
              <Textarea
                aria-label={`Comment for #${c.id} (edit)`}
                className="h-24 w-full"
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
              />
              <div className="flex items-center gap-2">
                <Button
                  size="sm"
                  onClick={() => {
                    onSave(draft);
                    setEditing(false);
                  }}
                >
                  Save comment
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    setDraft(note);
                    setEditing(false);
                  }}
                >
                  Cancel
                </Button>
              </div>
            </>
          ) : (
            <>
              <p className="whitespace-pre-wrap text-sm text-text">{note}</p>
              <div className="flex items-center gap-2 pt-1">
                <Button variant="outline" size="sm" onClick={() => setEditing(true)}>
                  Edit
                </Button>
                <Button
                  variant="danger"
                  size="sm"
                  onClick={() => {
                    // Saving empty deletes the note; the chip disappears too.
                    onSave("");
                    onClose();
                  }}
                >
                  Remove
                </Button>
                <span className="text-[11px] text-faint">
                  Saved on this device only — never sent to Azure DevOps.
                </span>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

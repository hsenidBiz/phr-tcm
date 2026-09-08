import { MessageSquarePlus } from "lucide-react";
import { useState } from "react";
import type { TestCaseFull } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Textarea } from "../../components/ui/input";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";

/** The expanded (read-only) detail: preconditions, steps, and the personal
 * local comment - a scratchpad saved on this machine only, never written
 * to Azure DevOps. */
export default function CaseDetail({
  c,
  note,
  onSaveNote,
}: {
  c: TestCaseFull;
  note: string;
  onSaveNote: (text: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(note);

  return (
    <div className="border-t border-border" onClick={(e) => e.stopPropagation()}>
      {c.tags && (
        <div className="flex flex-wrap items-center gap-1 border-b border-border/60 px-3 py-2">
          {c.tags
            .split(";")
            .map((t) => t.trim())
            .filter(Boolean)
            .map((t) => (
              <span
                key={t}
                className="rounded-full bg-surface-2 px-2 py-0.5 text-[11px] text-muted"
              >
                {t}
              </span>
            ))}
        </div>
      )}
      {c.preconditions && (
        <p className="whitespace-pre-wrap border-b border-border/60 px-3 py-2 text-xs text-muted">
          <span className="font-semibold">Preconditions: </span>
          {c.preconditions}
        </p>
      )}

      {c.steps.length > 0 ? (
        <table className="w-full border-collapse text-xs">
          <thead>
            <tr className="text-left text-faint">
              <th className="w-8 px-3 py-1 font-medium">#</th>
              <th className="px-3 py-1 font-medium">Action</th>
              <th className="px-3 py-1 font-medium">Expected</th>
            </tr>
          </thead>
          <tbody>
            {c.steps.map((s, i) => (
              <tr key={i} className="border-t border-border/40 align-top">
                <td className="px-3 py-1 text-faint">{i + 1}</td>
                <td className="whitespace-pre-wrap px-3 py-1 text-text">{s.action}</td>
                <td className="whitespace-pre-wrap px-3 py-1 text-muted">{s.expected}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <p className="px-3 py-2 text-xs text-muted">This test case has no steps.</p>
      )}

      <div className="border-t border-border/60 px-3 py-2">
        {editing ? (
          <div className="space-y-2">
            <Textarea
              aria-label={`Comment for #${c.id}`}
              className="h-20 w-full"
              placeholder="e.g. Step 3 needs the new confirmation dialog"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                disabled={draft === note}
                title={draft === note ? "Nothing changed yet" : undefined}
                onClick={() => {
                  onSaveNote(draft);
                  setEditing(false);
                }}
              >
                <IconConfirm aria-hidden />
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
                <IconCancel aria-hidden />
                Cancel
              </Button>
              <span className="text-[11px] text-faint">
                Saved locally
              </span>
            </div>
          </div>
        ) : note ? (
          <div className="flex items-start gap-2 rounded-md bg-accent-soft/60 px-2.5 py-1.5">
            <p className="whitespace-pre-wrap text-xs text-text">{note}</p>
            <button
              className="ml-auto shrink-0 text-xs text-accent hover:underline"
              onClick={() => {
                setDraft(note);
                setEditing(true);
              }}
            >
              Edit
            </button>
          </div>
        ) : (
          <button
            className="flex items-center gap-1.5 rounded px-1 py-0.5 text-xs text-faint transition-colors hover:text-accent"
            onClick={() => {
              setDraft("");
              setEditing(true);
            }}
          >
            <MessageSquarePlus size={13} />
            Add comment
          </button>
        )}
      </div>
    </div>
  );
}

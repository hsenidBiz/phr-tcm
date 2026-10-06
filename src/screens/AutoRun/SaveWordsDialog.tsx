// The words that make a request a save, for scripts marked Must not save.
//
// A no-save script's browser stops a POST, PUT, PATCH or DELETE whose
// address path holds one of these words. The built-in words are fixed and
// shown only so the person knows what is already covered; the project's
// own are added and removed here and saved as one list, through
// `auto_run_set_save_words`, which checks every word (Rust says why one is
// refused, and nothing is written). The list lives in the project's Auto
// Run settings file, beside its areas, so it shares their query key.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { commands, type NavView } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { IconAdd, IconCancel, IconConfirm, IconRemove } from "../../lib/actionIcons";
import { toast } from "../../lib/toast";

/** The built-in words, for a view written before the app kept them there. */
export const BUILT_IN_SAVE_WORDS = ["save", "update", "delete", "submit", "approve", "publish", "assign"];

export default function SaveWordsDialog({
  org,
  project,
  view,
  onClose,
}: {
  org: string;
  project: string;
  /** The project's settings as last read; `null` when it has none yet. */
  view: NavView | null;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const builtIn = view?.built_in_save_words?.length ? view.built_in_save_words : BUILT_IN_SAVE_WORDS;
  const [words, setWords] = useState<string[]>(view?.save_words ?? []);
  const [draft, setDraft] = useState("");
  const [problem, setProblem] = useState("");
  const headingId = useId();

  /** The list with the typed word added, or why it cannot be. A word typed
   * and not yet added when Save is pressed is added - what the person sees
   * in the box is what gets saved. */
  const withDraft = (): string[] | string => {
    const word = draft.trim().toLowerCase();
    if (!word) return words;
    if (builtIn.includes(word)) return `"${word}" is already a built-in save word`;
    return words.includes(word) ? words : [...words, word];
  };

  const add = () => {
    const next = withDraft();
    if (typeof next === "string") {
      setProblem(next);
      return;
    }
    setProblem("");
    setWords(next);
    setDraft("");
  };

  const save = useMutation({
    mutationFn: async (list: string[]) => {
      const res = await commands.autoRunSetSaveWords(org, project, list);
      if (res.status === "error") throw new Error(res.error);
      return res.data;
    },
    onSuccess: (next) => {
      qc.setQueryData(["autorun-nav", org, project], next);
      toast.success("Save words saved.");
      onClose();
    },
    onError: (e) => setProblem(e instanceof Error ? e.message : String(e)),
  });

  return (
    <Modal onClose={onClose} labelledBy={headingId} className="flex w-full max-w-2xl flex-col gap-3 p-5">
      <h2 id={headingId} className="text-sm font-semibold text-text">
        Save words
      </h2>
      <p className="text-xs text-muted">
        A script marked Must not save has a request stopped before it reaches the server when it is a
        POST, PUT, PATCH or DELETE whose address path holds one of these words. Case does not matter,
        and the query is not read.
      </p>
      <div className="space-y-1">
        <span className="text-xs font-medium text-muted">Built in</span>
        <ul aria-label="Built-in save words" className="flex flex-wrap gap-1.5">
          {builtIn.map((w) => (
            <li key={w} className="id-mono rounded border border-border px-1.5 py-0.5 text-xs text-muted">
              {w}
            </li>
          ))}
        </ul>
      </div>
      <div className="space-y-1">
        <span className="text-xs font-medium text-muted">This project&apos;s own</span>
        {words.length === 0 ? (
          <p className="text-xs text-faint">None yet.</p>
        ) : (
          <ul aria-label="This project's save words" className="space-y-1">
            {words.map((w) => (
              <li key={w} className="flex items-center justify-between gap-2">
                <span className="id-mono text-sm text-text">{w}</span>
                <Button
                  size="sm"
                  variant="ghost"
                  aria-label={`Remove save word ${w}`}
                  onClick={() => setWords(words.filter((x) => x !== w))}
                >
                  <IconRemove aria-hidden />
                  Remove
                </Button>
              </li>
            ))}
          </ul>
        )}
        <div className="flex gap-2">
          <Input
            aria-label="New save word"
            className="flex-1"
            placeholder="recalculate"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                add();
              }
            }}
          />
          <Button size="sm" variant="outline" disabled={draft.trim() === ""} onClick={add}>
            <IconAdd aria-hidden />
            Add
          </Button>
        </div>
      </div>
      {problem && <p className="text-xs text-danger">{problem}</p>}
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button
          size="sm"
          disabled={save.isPending}
          onClick={() => {
            const next = withDraft();
            if (typeof next === "string") {
              setProblem(next);
              return;
            }
            setProblem("");
            save.mutate(next);
          }}
        >
          <IconConfirm aria-hidden />
          {save.isPending ? "Saving" : "Save"}
        </Button>
      </div>
    </Modal>
  );
}

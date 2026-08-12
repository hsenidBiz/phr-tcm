// Moving test cases to a different PBI - the fix for cases that landed in
// the wrong one. Reversible by design (the same move with the PBIs
// swapped), so unlike DeleteConfirm there is no acknowledgement gate -
// but the confirmation still lists every case by id and title, because
// "move 12 test cases" is not something anyone can check.

import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { commands, type PbiHit, type RelinkOutcome, type TestCaseFull } from "../bindings";
import { describeAdoError, unwrap } from "../lib/ipc";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Modal } from "./ui/modal";
import { IconCancel, IconConfirm } from "../lib/actionIcons";

export default function RelinkDialog({
  org,
  project,
  fromPbi,
  cases,
  onClose,
  onMoved,
}: {
  org: string;
  project: string;
  /** The PBI the cases are being moved OUT of - the one on screen. */
  fromPbi: number;
  cases: TestCaseFull[];
  onClose: () => void;
  /** Called when at least one case moved, so the list refreshes. */
  onMoved: () => void;
}) {
  const [text, setText] = useState("");
  const [query, setQuery] = useState("");
  const [target, setTarget] = useState<PbiHit | null>(null);
  const [failures, setFailures] = useState<RelinkOutcome[] | null>(null);
  const [movedCount, setMovedCount] = useState(0);

  // Same debounced search-as-you-type as the context bar's PbiPicker.
  useEffect(() => {
    const t = text.trim();
    if (!t) {
      setQuery("");
      return;
    }
    const h = setTimeout(() => setQuery(t), 250);
    return () => clearTimeout(h);
  }, [text]);

  const hits = useQuery({
    queryKey: ["pbi-search", org, project, query],
    queryFn: () => unwrap(commands.searchPbis(org, project, query)),
    enabled: query.length > 0,
    retry: false,
  });

  const move = useMutation({
    mutationFn: () =>
      unwrap(
        commands.relinkTestCases(
          org,
          project,
          cases.map((c) => c.id),
          fromPbi,
          target!.id,
        ),
      ),
    onSuccess: (outcomes) => {
      const failed = outcomes.filter((o) => !o.moved);
      const moved = outcomes.length - failed.length;
      if (failed.length === 0) {
        toast.success(
          `Moved ${moved} test case${moved === 1 ? "" : "s"} to #${target!.id}.`,
        );
        onMoved();
        onClose();
        return;
      }
      // A partial move refreshes what DID move, then stays open to show
      // exactly which cases were left behind and why.
      setMovedCount(moved);
      setFailures(failed);
      if (moved > 0) onMoved();
      toast.warning(`${moved} moved, ${failed.length} could not be.`, { duration: 20000 });
    },
    onError: (e) => toast.error(`Could not move the cases: ${e.message}`),
  });

  if (failures) {
    return (
      <Modal onClose={onClose} className="w-full max-w-lg space-y-3 p-5">
        <h2 className="text-sm font-semibold text-danger">
          {failures.length} could not be moved
        </h2>
        <p className="text-xs text-muted">
          {movedCount > 0
            ? `The other ${movedCount} now live under #${target?.id}. These stayed exactly where they were.`
            : "Nothing was moved - every case stayed exactly where it was."}
        </p>
        <ul className="max-h-60 space-y-1 overflow-auto rounded-md border border-border p-2 text-xs">
          {failures.map((f) => (
            <li key={f.id} className="flex gap-2 px-1 py-0.5">
              <span className="id-mono shrink-0 text-faint">#{f.id}</span>
              <span className="text-danger">{f.error ? describeAdoError(f.error) : "unknown"}</span>
            </li>
          ))}
        </ul>
        <div className="flex justify-end">
          <Button size="sm" onClick={onClose}>
            Close
          </Button>
        </div>
      </Modal>
    );
  }

  return (
    <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-lg flex-col gap-3 p-5">
      <div className="shrink-0">
        <h2 className="text-sm font-semibold text-text">
          Move {cases.length} test case{cases.length === 1 ? "" : "s"} to another PBI
        </h2>
        <p className="mt-1 text-xs text-muted">
          The Tested By link moves from <span className="id-mono">#{fromPbi}</span> to the PBI you
          pick - the cases leave this list and appear under the new PBI's suite. Reversible:
          moving them back is the same operation.
        </p>
      </div>

      {!target ? (
        <div className="min-h-0 flex-1 space-y-2 overflow-auto">
          <Input
            aria-label="Search for the destination PBI"
            autoFocus
            placeholder="Search PBIs by title or id"
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
          {hits.isLoading && query && <p className="text-xs text-faint">Searching…</p>}
          {(hits.data ?? [])
            .filter((h) => h.id !== fromPbi)
            .map((h) => (
              <button
                key={h.id}
                className="flex w-full items-center gap-2 rounded-md border border-border bg-surface px-3 py-1.5 text-left text-sm transition-colors hover:border-accent"
                onClick={() => setTarget(h)}
              >
                <span className="id-mono shrink-0 text-faint">#{h.id}</span>
                <span className="truncate text-text">{h.title}</span>
              </button>
            ))}
          {hits.data && hits.data.filter((h) => h.id !== fromPbi).length === 0 && query && (
            <p className="text-xs text-faint">No other PBI matches "{query}".</p>
          )}
        </div>
      ) : (
        <>
          <div className="shrink-0 rounded-md border border-accent/40 bg-accent-soft px-3 py-2 text-sm">
            <span className="id-mono text-faint">#{target.id}</span>{" "}
            <span className="text-text">{target.title}</span>
            <button
              className="ml-2 text-xs text-muted underline hover:text-accent"
              onClick={() => setTarget(null)}
            >
              change
            </button>
          </div>
          <ul className="min-h-0 flex-1 space-y-1 overflow-auto rounded-md border border-border p-2 text-xs">
            {cases.map((c) => (
              <li key={c.id} className="flex gap-2 px-1 py-0.5">
                <span className="id-mono shrink-0 text-faint">#{c.id}</span>
                <span className="text-text">{c.title}</span>
              </li>
            ))}
          </ul>
        </>
      )}

      <div className="flex shrink-0 justify-end gap-2">
        <Button variant="ghost" size="sm" disabled={move.isPending} onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button
          size="sm"
          disabled={!target || move.isPending || cases.length === 0}
          onClick={() => move.mutate()}
        >
          <IconConfirm aria-hidden />
          {move.isPending ? "Moving" : target ? `Move ${cases.length} to #${target.id}` : "Pick a PBI first"}
        </Button>
      </div>
    </Modal>
  );
}

// Find and replace on the open report. Find lists where the text is, in the
// report and in its subreports. Replace all first asks the report what it
// would change (a dry run), shows the count, and only saves when the person
// says yes: a saved replace is an ordinary edit with no undo of its own.

import { useState } from "react";
import { commands, type FindHit, type ReplaceOutcome } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Input } from "../../components/ui/input";
import { IconSearch } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

function skippedNote(n: number): string {
  if (n === 0) return "";
  return n === 1 ? " 1 skipped: its text holds fields." : ` ${n} skipped: their text holds fields.`;
}

export function FindReplace({ onChanged }: { onChanged: () => void }) {
  const [find, setFind] = useState("");
  const [replace, setReplace] = useState("");
  const [matchCase, setMatchCase] = useState(false);
  const [hits, setHits] = useState<FindHit[] | null>(null);
  const [pending, setPending] = useState<ReplaceOutcome | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const start = () => {
    setError(null);
    setDone(null);
    setPending(null);
    setBusy(true);
  };

  const runFind = async () => {
    start();
    try {
      setHits(await unwrapStr(commands.crystalFind(find, matchCase)));
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  // The dry run: what would change, and nothing saved.
  const review = async () => {
    start();
    try {
      const out = await unwrapStr(commands.crystalReplace(find, replace, matchCase, true));
      if (out.replaced === 0) {
        setDone(`Nothing to replace.${skippedNote(out.skipped_locked)}`);
      } else {
        setPending(out);
      }
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    setError(null);
    setBusy(true);
    try {
      const out = await unwrapStr(commands.crystalReplace(find, replace, matchCase, false));
      setPending(null);
      setHits(null);
      setDone(`Replaced ${out.replaced}.${skippedNote(out.skipped_locked)}`);
      onChanged();
    } catch (e) {
      setPending(null);
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const empty = find === "";
  return (
    <section aria-label="Find and replace" className="shrink-0 rounded-lg border border-border bg-surface px-4 py-3">
      <h2 className="mb-2 text-sm font-semibold text-text">Find and replace</h2>
      <div className="grid gap-2 sm:grid-cols-2">
        <Input aria-label="Find" className="py-1.5" value={find} placeholder="Find" onChange={(e) => setFind(e.target.value)} />
        <Input
          aria-label="Replace with"
          className="py-1.5"
          value={replace}
          placeholder="Replace with"
          onChange={(e) => setReplace(e.target.value)}
        />
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-3">
        <label className="flex items-center gap-2 text-xs text-text">
          <Checkbox checked={matchCase} onCheckedChange={setMatchCase} ariaLabel="Match case" />
          Match case
        </label>
        <Button size="sm" variant="outline" disabled={empty || busy} onClick={() => void runFind()}>
          <IconSearch aria-hidden />
          Find
        </Button>
        <Button size="sm" variant="outline" disabled={empty || busy} onClick={() => void review()}>
          Replace all
        </Button>
      </div>
      {pending && (
        <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-text">
          <span>
            Replace {plural(pending.replaced, "text object", "text objects")}?
            {pending.changes.some((c) => c.subreport) && " Some are in subreports."} This is saved at once.
            {skippedNote(pending.skipped_locked)}
          </span>
          <Button size="sm" disabled={busy} onClick={() => void apply()}>
            Yes, replace
          </Button>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => setPending(null)}>
            Cancel
          </Button>
        </div>
      )}
      {done && <p className="mt-2 text-xs text-text">{done}</p>}
      {error && (
        <p role="alert" className="mt-2 text-xs text-danger">
          {error}
        </p>
      )}
      {hits && (
        <div className="mt-2">
          <p className="text-xs text-muted">{hits.length === 0 ? "No matches." : plural(hits.length, "match", "matches")}</p>
          <ul className="mt-1 flex max-h-48 flex-col gap-1 overflow-y-auto">
            {hits.map((h) => (
              <li
                key={`${h.subreport ?? ""}/${h.object_id}`}
                className="flex min-w-0 flex-wrap items-center gap-x-2 text-xs text-text"
              >
                <span className="font-medium">{h.name || h.kind}</span>
                <span className="text-muted">{h.section}</span>
                {h.subreport && <span className="text-muted">Subreport: {h.subreport}</span>}
                <span className="min-w-0 truncate">{h.text}</span>
                {!h.replaceable && <Badge>Has fields, cannot be replaced</Badge>}
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}

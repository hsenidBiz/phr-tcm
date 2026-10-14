// Finish in the repo: under an export's result (the Export dialog and Sync),
// one ready-to-paste prompt per test-case file for Claude Code opened in the
// person's clone. It runs steps 4, 5, 6 and 7 of the repo's /gen-test (run
// and heal the raw specs, which the export never runs, then refactor, lint
// and live check, run summary) on exactly the cases just written, into the
// spec named like the test-case file. Rust builds the text from checked
// values (pw_repo::handoff); this only shows it and copies it. Nothing is
// started.
//
// A card whose cases cannot share one spec's user shows only why. A case a
// spec already has is left out of the prompt and listed: /edit-spec changes
// it, not this.

import { useEffect, useState } from "react";
import { commands, type ExportResult, type HandoffCard } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Textarea } from "../../components/ui/input";
import { IconConfirm, IconCopy } from "../../lib/actionIcons";
import { copyText } from "../../lib/clipboard";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The one line under the heading. */
export const HANDOFF_NOTE =
  "Paste this into Claude Code opened in your clone. It runs the raw specs, the repo's refactor, lint and live check.";

function Card({ card }: { card: HandoffCard }) {
  const [copied, setCopied] = useState(false);
  const label = `${card.seg}/${card.stem}`;

  const copy = async () => {
    try {
      await copyText(card.prompt);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      toast.error("Could not copy the prompt.");
    }
  };

  const already =
    card.already.length > 0 ? (
      <ul className="space-y-0.5 text-xs text-muted">
        {card.already.map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
    ) : null;

  if (card.problem) {
    return (
      <div className="space-y-1.5 rounded-lg border border-warning/40 bg-warning/10 p-3" aria-label={`Finish ${label}`}>
        <p className="font-mono text-xs text-muted">{label}</p>
        <p className="text-xs text-warning">{card.problem}</p>
        {already}
      </div>
    );
  }

  return (
    <div className="space-y-2 rounded-lg border border-border bg-surface p-3" aria-label={`Finish ${label}`}>
      {card.prompt && (
        <>
          <p className="text-xs text-text">{`${card.seg}: ${card.case_ids.map((id) => `#${id}`).join(", ")} into specs/${card.stem}.spec.ts`}</p>
          <div className="relative">
            <Textarea
              aria-label={`Prompt for ${label}`}
              readOnly
              className="h-36 w-full resize-y bg-bg pr-24 font-mono text-xs"
              value={card.prompt}
            />
            <Button
              size="sm"
              variant="outline"
              className="absolute top-2 right-4 bg-surface"
              aria-label={`Copy the prompt for ${label}`}
              onClick={() => void copy()}
            >
              {copied ? <IconConfirm aria-hidden /> : <IconCopy aria-hidden />}
              {copied ? "Copied" : "Copy"}
            </Button>
          </div>
        </>
      )}
      {already}
    </div>
  );
}

export default function FinishInRepo({
  clone,
  org,
  project,
  result,
}: {
  clone: string;
  org: string;
  project: string;
  /** What the export just wrote: its cases are the ones finished. */
  result: ExportResult;
}) {
  const ids = result.cases.map(([id]) => id);
  const idsKey = ids.join(",");
  const [cards, setCards] = useState<HandoffCard[]>([]);
  const [error, setError] = useState("");

  useEffect(() => {
    if (ids.length === 0) {
      setCards([]);
      setError("");
      return;
    }
    let live = true;
    void (async () => {
      try {
        const next = await unwrapStr(commands.pwHandoff(clone, org, project, ids));
        if (!live) return;
        setCards(next ?? []);
        setError("");
      } catch (e) {
        if (!live) return;
        // An old export's cards must not stand beside this one's refusal.
        setCards([]);
        setError(message(e));
      }
    })();
    return () => {
      live = false;
    };
    // `ids` is read through `idsKey`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clone, org, project, idsKey]);

  if (ids.length === 0) return null;

  return (
    <section className="space-y-2 border-t border-border pt-3" aria-label="Finish in the repo">
      <h3 className="text-sm font-semibold text-text">Finish in the repo</h3>
      <p className="text-xs text-muted">{HANDOFF_NOTE}</p>
      {error && <p className="whitespace-pre-wrap text-xs text-danger">{error}</p>}
      {cards.map((card) => (
        <Card key={`${card.seg}/${card.stem}`} card={card} />
      ))}
    </section>
  );
}

// Finish in the repo: under an export's result (the Export dialog and Sync),
// one ready-to-paste prompt per feature folder (<seg>) for Claude Code opened
// in the person's clone. It runs steps 4, 5, 6 and 7 of the repo's /gen-test
// (run and heal the raw specs, which the export never runs, then refactor,
// lint and live check, run summary) on exactly the cases just
// written. Rust builds the text from checked values (pw_repo::handoff); this
// only shows it, offers the spec to put the cases in, and copies it. Nothing
// is started.

import { useEffect, useState } from "react";
import { commands, type ExportResult, type HandoffCard, type HandoffChoice } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Input, Textarea } from "../../components/ui/input";
import { Select } from "../../components/ui/select";
import { IconConfirm, IconCopy } from "../../lib/actionIcons";
import { copyText } from "../../lib/clipboard";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";

const sectionTitle = "text-xs font-semibold uppercase tracking-wide text-muted";
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The one line under the heading. */
export const HANDOFF_NOTE =
  "Paste this into Claude Code opened in your clone. It runs the raw specs, the repo's refactor, lint and live check.";

/** The rule Rust checks a typed spec name against (pw_repo::handoff::valid_stem). */
const STEM = /^[a-z0-9][a-z0-9-]*$/;
const ONE_PER_CASE = "one";
const NEW_SPEC = "new";
const stemValue = (stem: string) => `stem:${stem}`;

/** What the picker shows for a card. */
function picked(card: HandoffCard, typing: boolean): string {
  if (typing) return NEW_SPEC;
  if (card.stem == null) return ONE_PER_CASE;
  return card.stems.includes(card.stem) ? stemValue(card.stem) : NEW_SPEC;
}

function Card({
  card,
  typed,
  onChoose,
  onType,
}: {
  card: HandoffCard;
  /** The new spec name being typed, or null when an existing choice is picked. */
  typed: string | null;
  onChoose: (stem: string | null) => void;
  onType: (name: string) => void;
}) {
  const [copied, setCopied] = useState(false);
  const typing = typed !== null;
  const badName = typing && typed.trim() !== "" && !STEM.test(typed.trim());

  const copy = async () => {
    try {
      await copyText(card.prompt);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      toast.error("Could not copy the prompt.");
    }
  };

  return (
    <div className="space-y-1.5" aria-label={`Finish ${card.seg}`}>
      <p className="text-xs text-text">{`${card.seg}: ${card.case_ids.map((id) => `#${id}`).join(", ")}`}</p>
      <div className="flex flex-wrap items-center gap-2">
        <Select
          aria-label={`Spec for ${card.seg}`}
          className="w-72"
          value={picked(card, typing)}
          onChange={(e) => {
            const v = e.target.value;
            if (v === NEW_SPEC) onType("");
            else if (v === ONE_PER_CASE) onChoose(null);
            else onChoose(v.slice("stem:".length));
          }}
        >
          {card.stems.map((s) => (
            <option key={s} value={stemValue(s)}>{`Add to ${s}.spec.ts`}</option>
          ))}
          <option value={ONE_PER_CASE}>One spec per case</option>
          <option value={NEW_SPEC}>New spec…</option>
        </Select>
        {typing && (
          <Input
            aria-label={`New spec name for ${card.seg}`}
            placeholder="spec-name"
            className="w-56"
            value={typed}
            onChange={(e) => onType(e.target.value)}
          />
        )}
        {badName && <span className="text-xs text-danger">Use lower-case letters, digits and hyphens</span>}
      </div>
      <Textarea
        aria-label={`Prompt for ${card.seg}`}
        readOnly
        className="h-36 w-full font-mono text-xs"
        value={card.prompt}
      />
      <div className="flex justify-end">
        <Button size="sm" variant="outline" aria-label={`Copy the prompt for ${card.seg}`} onClick={() => void copy()}>
          {copied ? <IconConfirm aria-hidden /> : <IconCopy aria-hidden />}
          {copied ? "Copied" : "Copy"}
        </Button>
      </div>
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
  const [choices, setChoices] = useState<HandoffChoice[]>([]);
  /** A new spec name being typed, by seg: sent once it follows the rule. */
  const [typed, setTyped] = useState<Record<string, string>>({});
  const [cards, setCards] = useState<HandoffCard[]>([]);
  const [error, setError] = useState("");

  // A new export is a new set of cases: its choices start from the defaults.
  useEffect(() => {
    setChoices([]);
    setTyped({});
  }, [idsKey]);

  useEffect(() => {
    if (ids.length === 0) {
      setCards([]);
      return;
    }
    let live = true;
    void (async () => {
      try {
        const next = await unwrapStr(commands.pwHandoff(clone, org, project, ids, choices));
        if (!live) return;
        setCards(next ?? []);
        setError("");
      } catch (e) {
        if (live) setError(message(e));
      }
    })();
    return () => {
      live = false;
    };
    // `ids` is read through `idsKey`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clone, org, project, idsKey, choices]);

  if (ids.length === 0) return null;

  const choose = (seg: string, stem: string | null) => {
    setTyped((cur) => {
      const next = { ...cur };
      delete next[seg];
      return next;
    });
    setChoices((cur) => [...cur.filter((c) => c.seg !== seg), { seg, stem }]);
  };
  const type = (seg: string, name: string) => {
    setTyped((cur) => ({ ...cur, [seg]: name }));
    const n = name.trim();
    if (STEM.test(n)) setChoices((cur) => [...cur.filter((c) => c.seg !== seg), { seg, stem: n }]);
  };

  return (
    <section className="space-y-2 border-t border-border pt-3" aria-label="Finish in the repo">
      <h3 className={sectionTitle}>Finish in the repo</h3>
      <p className="text-xs text-muted">{HANDOFF_NOTE}</p>
      {error && <p className="whitespace-pre-wrap text-xs text-danger">{error}</p>}
      {cards.map((card) => (
        <Card
          key={card.seg}
          card={card}
          typed={card.seg in typed ? typed[card.seg] : null}
          onChoose={(stem) => choose(card.seg, stem)}
          onType={(name) => type(card.seg, name)}
        />
      ))}
    </section>
  );
}

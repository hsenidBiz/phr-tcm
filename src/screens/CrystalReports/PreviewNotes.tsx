// The preview's notes in plain words (see ./notes for how they are sorted):
// the fields that had no data as one card with the names behind Show
// fields, the symbols drawn from a fallback font as one quiet line, and
// everything else behind Other notes.

import { useId, useState, type ReactNode } from "react";
import { Button } from "../../components/ui/button";
import { IconHideDetails, IconShowDetails } from "../../lib/actionIcons";
import { classifyNotes, missingFieldsBody, missingFieldsHeadline, symbolsNote } from "./notes";

/** A button that shows and hides the list under it. */
function Toggle({ open, controls, onClick, children }: { open: boolean; controls: string; onClick: () => void; children: ReactNode }) {
  return (
    <Button size="sm" variant="ghost" className="-ml-2 px-2" aria-expanded={open} aria-controls={controls} onClick={onClick}>
      {open ? <IconHideDetails aria-hidden /> : <IconShowDetails aria-hidden />}
      {children}
    </Button>
  );
}

export function PreviewNotes({
  lines,
  live,
  otherOpenAtFirst,
}: {
  lines: readonly string[];
  /** The live preview's connection name, or null for saved data. */
  live: string | null;
  /** Show the other notes without being asked - when nothing was drawn, they say why. */
  otherOpenAtFirst: boolean;
}) {
  const [fieldsOpen, setFieldsOpen] = useState(false);
  const [otherChoice, setOtherChoice] = useState<boolean | null>(null);
  const fieldsId = useId();
  const otherId = useId();
  const { fields, glyphs, other } = classifyNotes(lines);
  if (fields.length === 0 && glyphs.length === 0 && other.length === 0) return null;
  const otherOpen = otherChoice ?? otherOpenAtFirst;

  return (
    <section aria-label="Preview notes" className="shrink-0 space-y-2 rounded-lg border border-border bg-surface px-4 py-3">
      {fields.length > 0 && (
        <div className="space-y-1">
          <p className="text-sm font-medium text-text">{missingFieldsHeadline(fields.length)}</p>
          <p className="text-xs text-muted">{missingFieldsBody(live)}</p>
          <Toggle open={fieldsOpen} controls={fieldsId} onClick={() => setFieldsOpen((o) => !o)}>
            {fieldsOpen ? "Hide fields" : "Show fields"}
          </Toggle>
          {fieldsOpen && (
            <ul id={fieldsId} className="max-h-40 space-y-0.5 overflow-y-auto pl-1 text-xs">
              {fields.map((f) => (
                <li key={f.raw} title={f.raw} className="flex flex-wrap items-baseline gap-x-2">
                  <span className="text-text">{f.name}</span>
                  <span className="font-mono text-[11px] text-faint">{f.raw}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
      {glyphs.length > 0 && <p className="text-xs text-muted">{symbolsNote(glyphs)}</p>}
      {other.length > 0 && (
        <div>
          <Toggle open={otherOpen} controls={otherId} onClick={() => setOtherChoice(!otherOpen)}>
            {`Other notes (${other.length})`}
          </Toggle>
          {otherOpen && (
            <ul id={otherId} className="max-h-40 list-disc space-y-0.5 overflow-y-auto pl-5 text-xs text-muted">
              {other.map((line, i) => (
                <li key={i}>{line}</li>
              ))}
            </ul>
          )}
        </div>
      )}
    </section>
  );
}

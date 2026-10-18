// Test case text - a step's action or expected result, or the List of
// Prerequisites - shown formatted, and edited as the Markdown it is.
//
// Every place the app shows case text goes through `CaseText`, so all of
// them render the one dialect the converter writes to Azure DevOps (see
// `renderCaseMarkdown`): what a person sees here is what the work item
// shows.

import { useMemo, type TextareaHTMLAttributes } from "react";
import { IconPreview } from "../lib/actionIcons";
import { cn } from "../lib/cn";
import { renderCaseMarkdown, type CaseTextKind } from "../lib/markdown";
import { Button } from "./ui/button";
import { Textarea } from "./ui/input";

export default function CaseText({
  text,
  kind,
  className,
  empty,
}: {
  text: string;
  kind: CaseTextKind;
  className?: string;
  /** Shown, faint, when the text is blank; without it a blank text shows nothing. */
  empty?: string;
}) {
  const html = useMemo(() => renderCaseMarkdown(text, kind), [text, kind]);
  if (!text.trim()) {
    return empty ? <p className={cn("italic text-faint", className)}>{empty}</p> : null;
  }
  return (
    <div
      className={cn("md-preview case-text break-words", className)}
      // Sanitised in renderCaseMarkdown - case text comes from Azure DevOps
      // and from import files, and lib/markdown.ts says why that matters.
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

/**
 * A case text editor: a multi-line text area holding the Markdown exactly
 * as typed, or - with `preview` on - the same text formatted, in a box the
 * size of the field. The toggle is the caller's (`PreviewToggle`), so one
 * toggle can switch a whole step list.
 */
export function CaseTextField({
  kind,
  value,
  onChange,
  preview,
  className,
  ...props
}: Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "value" | "onChange"> & {
  kind: CaseTextKind;
  value: string;
  onChange: (value: string) => void;
  preview: boolean;
}) {
  if (preview) {
    return (
      <div
        className={cn(
          "min-h-9 min-w-0 rounded-md border border-border/60 bg-bg px-2 py-1.5 text-xs text-text",
          className,
          // The field's own height would clip a long preview; the preview
          // grows with its text instead.
          "h-auto",
        )}
      >
        <CaseText text={value} kind={kind} empty="Nothing to preview" />
      </div>
    );
  }
  return (
    <Textarea
      className={className}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      {...props}
    />
  );
}

/** Switches case text editors between the Markdown and the formatted
 * preview. `what` names the text it previews ("steps", "preconditions"),
 * so a form with two toggles has two distinct buttons. */
export function PreviewToggle({
  what,
  on,
  onToggle,
}: {
  what: string;
  on: boolean;
  onToggle: () => void;
}) {
  return (
    <Button
      variant="ghost"
      size="sm"
      aria-label={`Preview ${what}`}
      aria-pressed={on}
      title={on ? `Back to editing the ${what}` : `Show the ${what} formatted`}
      className={cn(on && "bg-accent-soft text-accent")}
      onClick={onToggle}
    >
      <IconPreview aria-hidden />
      Preview
    </Button>
  );
}

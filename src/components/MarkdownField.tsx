// A rich-text work item field, behaving the way Azure DevOps's own form
// does: it sits rendered until you click into it, then becomes an editor
// with a formatting toolbar and a live preview underneath.
//
// Rendered-by-default matters because reading is the common case - a
// drawer full of raw markdown is unreadable. Click-to-edit matters
// because the rendered block LOOKS like a text box, so people click it
// expecting to type.

import { Bold, Code, Italic, Link2, List, ListOrdered, Quote, Type } from "lucide-react";
import { useEffect, useRef } from "react";
import { cn } from "../lib/cn";
import { applyMd, type MdAction } from "../lib/markdownEdit";
import { Textarea } from "./ui/input";

const TOOLS: { action: MdAction; icon: typeof Bold; label: string }[] = [
  { action: "bold", icon: Bold, label: "Bold" },
  { action: "italic", icon: Italic, label: "Italic" },
  { action: "code", icon: Code, label: "Code" },
  { action: "link", icon: Link2, label: "Link" },
  { action: "heading", icon: Type, label: "Heading" },
  { action: "bullet", icon: List, label: "Bulleted list" },
  { action: "number", icon: ListOrdered, label: "Numbered list" },
  { action: "quote", icon: Quote, label: "Quote" },
];

export default function MarkdownField({
  label,
  value,
  onChange,
  editing,
  onStartEditing,
  renderHtml,
  rows = "h-28",
  previewMinHeight = "min-h-28",
  flagged,
}: {
  /** Used for the accessible names of the editor and the edit affordance. */
  label: string;
  value: string;
  onChange: (v: string) => void;
  editing: boolean;
  onStartEditing: () => void;
  /** Markdown -> sanitized HTML, supplied by the caller so attachment
   * URLs can be swapped for inline data first. */
  renderHtml: (md: string) => string;
  rows?: string;
  previewMinHeight?: string;
  flagged?: boolean;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const justOpened = useRef(false);

  // Clicking the rendered block should land the caret in the editor, not
  // just swap the widget - otherwise it takes two clicks to type.
  useEffect(() => {
    if (editing && justOpened.current) {
      justOpened.current = false;
      const el = ref.current;
      if (el) {
        el.focus();
        el.setSelectionRange(el.value.length, el.value.length);
      }
    }
  }, [editing]);

  const runTool = (action: MdAction) => {
    const el = ref.current;
    if (!el) return;
    const next = applyMd(el.value, el.selectionStart, el.selectionEnd, action);
    onChange(next.value);
    // Restore the selection after React re-renders with the new value,
    // so the user can keep typing (or hit the button again to toggle).
    requestAnimationFrame(() => {
      el.focus();
      el.setSelectionRange(next.start, next.end);
    });
  };

  if (!editing) {
    return (
      <div className="mt-1">
        <div
          // Not a <button>: rendered markdown can contain links, and
          // nesting those inside a button is invalid and unusable with a
          // screen reader. The keyboard route is the Edit button below.
          onClick={() => {
            justOpened.current = true;
            onStartEditing();
          }}
          title={`Click to edit ${label}`}
          className={cn(
            "md-preview w-full cursor-text rounded-md border border-border bg-bg px-3 py-2 text-sm text-text transition-colors hover:border-accent",
            previewMinHeight,
            flagged && "ring-2 ring-danger",
          )}
          dangerouslySetInnerHTML={{ __html: renderHtml(value) }}
        />
        <button
          aria-label={`Edit ${label}`}
          className="mt-1 text-[11px] text-faint underline-offset-2 hover:text-accent hover:underline"
          onClick={() => {
            justOpened.current = true;
            onStartEditing();
          }}
        >
          Edit
        </button>
      </div>
    );
  }

  return (
    <div className="mt-1 space-y-1">
      <div className="flex flex-wrap gap-0.5 rounded-md border border-border bg-surface-2 p-1">
        {TOOLS.map(({ action, icon: Icon, label: tip }) => (
          <button
            key={action}
            aria-label={tip}
            title={tip}
            className="rounded p-1 text-muted transition-colors hover:bg-surface hover:text-accent"
            // Keep the textarea's selection: a mousedown would blur it
            // first, and the action would apply to a collapsed caret.
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => runTool(action)}
          >
            <Icon size={13} />
          </button>
        ))}
      </div>
      <Textarea
        ref={ref}
        aria-label={`${label} (markdown)`}
        className={cn("w-full", rows, flagged && "ring-2 ring-danger")}
        placeholder="Supports markdown: **bold**, - lists, `code`, [links](url)"
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
      <div>
        <span className="text-[10px] font-semibold uppercase tracking-wide text-faint">
          Preview
        </span>
        <div
          className="md-preview mt-0.5 w-full rounded-md border border-border/60 bg-bg px-3 py-2 text-sm text-text"
          dangerouslySetInnerHTML={{ __html: renderHtml(value) }}
        />
      </div>
    </div>
  );
}

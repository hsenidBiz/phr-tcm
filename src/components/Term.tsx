import { useId, type ReactNode } from "react";
import { GLOSSARY, type TermName } from "../lib/glossary";

/**
 * A word the app uses without explaining, with its definition on hover and on
 * keyboard focus. Use it at the FIRST visible appearance of the word on a
 * screen, not on every occurrence.
 *
 * The bubble is the app's own tooltip (`TooltipLayer` turns every `title`
 * into one, and closes it on Escape), so there is one tooltip implementation.
 * The span is focusable so the keyboard reaches it, and while it is hovered or
 * focused `aria-describedby` points at the definition text from the start,
 * so a screen reader says it too. The text sits in a `hidden` element: it is
 * left out of the name of any heading or label the word is in, but a
 * description may still point at it.
 *
 * `within` is for a word inside a control (a tab, a button): a focusable
 * element in a button is no good, so the word is not a tab stop; give the
 * control `title={define(name)}` and the keyboard reaches the same text
 * through the control itself. The control's accessible name is unchanged
 * either way.
 */
export function Term({ name, within = false, children }: { name: TermName; within?: boolean; children: ReactNode }) {
  const id = useId();
  const text = GLOSSARY[name];
  return (
    <span
      title={text}
      data-tip-side="bottom"
      data-term={name}
      tabIndex={within ? undefined : 0}
      aria-describedby={within ? undefined : id}
      className="cursor-help underline decoration-dotted decoration-muted underline-offset-2 focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent motion-safe:transition-colors"
    >
      {children}
      {!within && (
        <span id={id} hidden>
          {text}
        </span>
      )}
    </span>
  );
}

/** The definition of a word, for a control's `title` where `Term` is `within` it. */
export function define(name: TermName): string {
  return GLOSSARY[name];
}

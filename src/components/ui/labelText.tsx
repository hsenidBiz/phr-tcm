import { Children, Fragment, isValidElement, type ReactElement, type ReactNode } from "react";

/** The class that centres a label by its letters (src/index.css, beside
 *  `.label-trim`, explains why). */
export const LABEL_TRIM = "label-trim";

/**
 * A control's children with every run of plain text wrapped in a
 * `label-trim` span, so a component (Button, Badge) centres its label by
 * its letters without each call site having to. Icons and other elements
 * pass through untouched; adjacent strings and numbers ("Run ", 2,
 * " cases") stay one span, so their spaces survive; fragments are opened
 * so the text inside them is reached too.
 */
export function trimLabels(children: ReactNode): ReactNode {
  const out: ReactNode[] = [];
  let run: (string | number)[] = [];
  const flush = () => {
    if (run.length === 0) return;
    if (run.some((t) => String(t).trim() !== "")) {
      out.push(
        <span key={`label-${out.length}`} className={LABEL_TRIM}>
          {run.join("")}
        </span>,
      );
    }
    run = [];
  };
  const visit = (nodes: ReactNode) => {
    Children.forEach(nodes, (child) => {
      if (typeof child === "string" || typeof child === "number") {
        run.push(child);
      } else if (isValidElement(child) && child.type === Fragment) {
        visit((child as ReactElement<{ children?: ReactNode }>).props.children);
      } else if (child !== null && child !== undefined && typeof child !== "boolean") {
        flush();
        out.push(isValidElement(child) && child.key === null ? <Fragment key={`node-${out.length}`}>{child}</Fragment> : child);
      }
    });
  };
  visit(children);
  flush();
  return out;
}

import { marked } from "marked";

/** Render markdown to HTML for display in a `.md-preview` container.
 * `breaks: true` matches Azure DevOps, which treats single newlines as
 * line breaks in work-item and PR descriptions. */
export function renderMarkdown(md: string): string {
  return marked.parse(md || "", { async: false, breaks: true }) as string;
}

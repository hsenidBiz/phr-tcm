import { openUrl } from "@tauri-apps/plugin-opener";
import { useMemo, type MouseEvent } from "react";
import { cn } from "../lib/cn";
import { renderMarkdown } from "../lib/markdown";
import { toast } from "../lib/toast";

/**
 * Read-only markdown written by someone else - a pull request's
 * description, a review comment - drawn through the app's one renderer
 * (`renderMarkdown`: marked, then DOMPurify's allowlist) in the compact
 * look of `.md-preview.md-compact`.
 *
 * A link leaves for the system browser and never navigates this webview:
 * one delegated handler on the container finds the anchor the click landed
 * in, whatever it was nested in (`<a><strong>..</strong></a>`).
 */
export default function MarkdownView({ text, className }: { text: string; className?: string }) {
  const html = useMemo(() => renderMarkdown(text), [text]);

  const onClick = (e: MouseEvent<HTMLDivElement>) => {
    const anchor = (e.target as Element).closest("a");
    if (!anchor || !e.currentTarget.contains(anchor)) return;
    e.preventDefault();
    const href = anchor.getAttribute("href");
    if (href) openUrl(href).catch(() => toast.error("Could not open the browser."));
  };

  return (
    <div
      className={cn("md-preview md-compact text-text", className)}
      onClick={onClick}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

import { openUrl } from "@tauri-apps/plugin-opener";
import { useLayoutEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { createPortal } from "react-dom";
import { IconCopy } from "../lib/actionIcons";
import { copyText } from "../lib/clipboard";
import { renderMarkdown } from "../lib/markdown";
import { cn } from "../lib/cn";
import { toast } from "../lib/toast";

/** A fenced block that named a language: the header bar sits on it. */
type CodeBar = { host: HTMLElement; language: string; code: HTMLElement };

function CopyBar({ language, code }: { language: string; code: HTMLElement }) {
  return (
    <>
      <span className="md-codebar-label">{language}</span>
      <button
        type="button"
        aria-label="Copy code"
        title="Copy code"
        className="md-codebar-copy"
        onClick={() =>
          copyText((code.textContent ?? "").replace(/\n$/, ""))
            .then(() => toast.success("Code copied."))
            .catch(() => toast.error("Could not copy to clipboard."))
        }
      >
        <IconCopy aria-hidden />
      </button>
    </>
  );
}

/**
 * Read-only markdown written by someone else - a pull request's
 * description, a review comment - drawn through the app's one renderer
 * (`renderMarkdown`: marked, then DOMPurify's allowlist) in the compact
 * look of `.md-preview.md-compact`. Single newlines run together, as they
 * always have in the pull request panel.
 *
 * A link leaves for the system browser and never navigates this webview:
 * one delegated handler on the container finds the anchor the click landed
 * in, whatever it was nested in (`<a><strong>..</strong></a>`).
 *
 * The sanitiser's allowlist has no buttons, so a code block's language
 * label and Copy button are added AFTER it, to the sanitised DOM: an empty
 * host element goes on top of each `pre` that names a language, and React
 * renders the bar into it through a portal. Nothing from the markdown ever
 * reaches the bar except the language text, which is set as text.
 */
export default function MarkdownView({ text, className }: { text: string; className?: string }) {
  const html = useMemo(() => renderMarkdown(text, { breaks: false }), [text]);
  const root = useRef<HTMLDivElement>(null);
  const [bars, setBars] = useState<CodeBar[]>([]);

  useLayoutEffect(() => {
    const el = root.current;
    if (!el) return;
    const hosts: HTMLElement[] = [];
    const found: CodeBar[] = [];
    el.querySelectorAll("pre").forEach((pre) => {
      const code = pre.querySelector<HTMLElement>(":scope > code");
      const language = /(?:^|\s)language-([\w+#.-]+)/.exec(code?.className ?? "")?.[1];
      if (!code || !language) return;
      const host = document.createElement("div");
      host.className = "md-codebar";
      pre.insertBefore(host, code);
      hosts.push(host);
      found.push({ host, language, code });
    });
    // An ordered list that starts above 1 counts from there (the marker is
    // a CSS counter, which does not read the start attribute itself).
    el.querySelectorAll<HTMLElement>("ol[start]").forEach((ol) => {
      const start = Number(ol.getAttribute("start"));
      if (Number.isFinite(start) && start !== 1) ol.style.counterReset = `md-ol ${start - 1}`;
    });
    setBars(found);
    return () => {
      for (const h of hosts) h.remove();
    };
  }, [html]);

  const onClick = (e: MouseEvent<HTMLDivElement>) => {
    const anchor = (e.target as Element).closest("a");
    if (!anchor || !e.currentTarget.contains(anchor)) return;
    e.preventDefault();
    const href = anchor.getAttribute("href");
    if (href) openUrl(href).catch(() => toast.error("Could not open the browser."));
  };

  return (
    <>
      <div
        ref={root}
        className={cn("md-preview md-compact text-text", className)}
        onClick={onClick}
        dangerouslySetInnerHTML={{ __html: html }}
      />
      {bars.map((b, i) => createPortal(<CopyBar language={b.language} code={b.code} />, b.host, `${i}`))}
    </>
  );
}

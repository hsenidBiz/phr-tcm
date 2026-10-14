import { useEffect, useLayoutEffect, useRef } from "react";
import { IconCancel } from "../lib/actionIcons";
import { cn } from "../lib/cn";
import { Button } from "./ui/button";
import { Modal } from "./ui/modal";

/**
 * A bundled page from public/vendor in the shared Modal, in an iframe.
 * Settings' optional extras open these (RunnerGameModal, BreakerGameModal).
 *
 * The frame is sandboxed (`sandbox="allow-scripts"`, no
 * `allow-same-origin`): on Windows, WebView2 injects Tauri's IPC scripts
 * into every subframe regardless of origin, so a same-origin frame would
 * carry the app's whole command surface. Sandboxing gives the frame an
 * opaque origin instead - Tauri's IPC then sees `Origin: null` and rejects
 * it, and the frame cannot reach `parent` except through `postMessage`. A
 * key pressed inside a frame never reaches this window either way, so each
 * page bundles an `escape.js` (ours) that relays Escape out as a message of
 * its own type, which is what closes this modal.
 *
 * Every message is checked against this frame's own window first; only
 * then does `escapeType` close it, and anything else go to `onFrameMessage`
 * (which must validate what it gets - the page is still untrusted input).
 */
export default function GameFrameModal({
  title,
  hint,
  src,
  escapeType,
  onClose,
  onFrameMessage,
  onFrameLoad,
  frameClassName,
}: {
  title: string;
  hint: string;
  src: string;
  escapeType: string;
  onClose: () => void;
  onFrameMessage?: (data: unknown) => void;
  /** Called with the frame's window each time it finishes loading. */
  onFrameLoad?: (win: Window) => void;
  /** Height classes for the frame. */
  frameClassName?: string;
}) {
  const frame = useRef<HTMLIFrameElement>(null);
  const close = useRef(onClose);
  const onMessage = useRef(onFrameMessage);
  const onLoad = useRef(onFrameLoad);

  useEffect(() => {
    close.current = onClose;
    onMessage.current = onFrameMessage;
    onLoad.current = onFrameLoad;
  }, [onClose, onFrameMessage, onFrameLoad]);

  useEffect(() => {
    const listener = (e: MessageEvent) => {
      const own = frame.current?.contentWindow;
      if (!own || e.source !== own) return;
      if ((e.data as { type?: unknown } | null)?.type === escapeType) {
        close.current();
        return;
      }
      onMessage.current?.(e.data);
    };
    window.addEventListener("message", listener);
    return () => window.removeEventListener("message", listener);
  }, [escapeType]);

  // Runs before the Modal's own cleanup (a parent's layout cleanup runs
  // before its children's), which copies the dialog for its close fade: the
  // copy then carries a blank frame rather than loading the page again.
  useLayoutEffect(() => {
    const el = frame.current;
    return () => {
      el?.setAttribute("src", "about:blank");
    };
  }, []);

  return (
    <Modal onClose={onClose} className="flex w-full max-w-2xl flex-col gap-3 p-5">
      <h2 className="text-sm font-semibold text-text">{title}</h2>
      <p className="text-xs text-muted">{hint}</p>
      {/* tabIndex makes it the Modal's first stop, so focus lands in the
          page. Tab/Shift+Tab pressed inside the frame is invisible to the
          focus trap for the same sandboxing reason Esc needs escape.js;
          the next parent-side Tab pulls focus back in either direction. */}
      <iframe
        ref={frame}
        title={title}
        src={src}
        sandbox="allow-scripts"
        tabIndex={0}
        referrerPolicy="no-referrer"
        onLoad={() => {
          const win = frame.current?.contentWindow;
          if (win) onLoad.current?.(win);
        }}
        className={cn("w-full rounded-md border border-border bg-surface", frameClassName ?? "h-80")}
      />
      <div className="flex justify-end">
        <Button size="sm" variant="outline" onClick={onClose}>
          <IconCancel aria-hidden />
          Close
        </Button>
      </div>
    </Modal>
  );
}

import { useEffect, useLayoutEffect, useRef } from "react";
import { IconCancel } from "../lib/actionIcons";
import { Button } from "./ui/button";
import { Modal } from "./ui/modal";

/** The bundled runner game (Chrome's offline T-Rex game) - see
 * public/vendor/runner/SOURCE.txt for where it came from and how it was
 * vetted. Settings' optional extras open it. */
export const RUNNER_SRC = "/vendor/runner/index.html";

/**
 * The game in the shared Modal, in an iframe of the bundled page.
 *
 * The frame is sandboxed (`sandbox="allow-scripts"`, no
 * `allow-same-origin`): on Windows, WebView2 injects Tauri's IPC scripts
 * into every subframe regardless of origin, so a same-origin frame would
 * carry the app's whole command surface. Sandboxing gives the frame an
 * opaque origin instead - Tauri's IPC then sees `Origin: null` and rejects
 * it, and the frame cannot reach `parent` except through `postMessage`. A
 * key pressed inside a frame never reaches this window either way, so
 * `escape.js` (bundled alongside the game, ours - see SOURCE.txt) relays
 * Escape out as a message, which is what closes this modal.
 */
export default function RunnerGameModal({ onClose }: { onClose: () => void }) {
  const frame = useRef<HTMLIFrameElement>(null);
  const close = useRef(onClose);

  useEffect(() => {
    close.current = onClose;
  }, [onClose]);

  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      if (e.source === frame.current?.contentWindow && (e.data as { type?: unknown } | null)?.type === "runner-escape") {
        close.current();
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);

  // Runs before the Modal's own cleanup (a parent's layout cleanup runs
  // before its children's), which copies the dialog for its close fade: the
  // copy then carries a blank frame rather than loading the game again.
  useLayoutEffect(() => {
    const el = frame.current;
    return () => {
      el?.setAttribute("src", "about:blank");
    };
  }, []);

  return (
    <Modal onClose={onClose} className="flex w-full max-w-2xl flex-col gap-3 p-5">
      <h2 className="text-sm font-semibold text-text">Dino game</h2>
      <p className="text-xs text-muted">Space or Up to jump, Down to duck. Esc closes the game.</p>
      {/* tabIndex makes it the Modal's first stop, so focus lands in the
          game. Tab/Shift+Tab pressed inside the frame is invisible to the
          focus trap for the same sandboxing reason Esc needs escape.js;
          the next parent-side Tab pulls focus back in either direction. */}
      <iframe
        ref={frame}
        title="Dino game"
        src={RUNNER_SRC}
        sandbox="allow-scripts"
        tabIndex={0}
        referrerPolicy="no-referrer"
        className="h-80 w-full rounded-md border border-border bg-surface"
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

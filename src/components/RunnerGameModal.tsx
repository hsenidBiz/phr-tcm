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
 * A key pressed inside a frame never reaches this window, so the Modal's
 * own Escape handler cannot hear it while the game has focus. The page is
 * served from the app's own origin, so a keydown listener goes straight
 * onto the frame's window instead - once per document the frame loads.
 */
export default function RunnerGameModal({ onClose }: { onClose: () => void }) {
  const frame = useRef<HTMLIFrameElement>(null);
  const close = useRef(onClose);
  const wired = useRef<Document | null>(null);

  useEffect(() => {
    close.current = onClose;
  }, [onClose]);

  // Runs before the Modal's own cleanup (a parent's layout cleanup runs
  // before its children's), which copies the dialog for its close fade: the
  // copy then carries a blank frame rather than loading the game again.
  useLayoutEffect(() => {
    const el = frame.current;
    return () => {
      el?.setAttribute("src", "about:blank");
    };
  }, []);

  const onLoad = () => {
    const win = frame.current?.contentWindow;
    if (!win) return;
    try {
      if (wired.current === win.document) return;
      wired.current = win.document;
      win.addEventListener("keydown", (e) => {
        if (e.key === "Escape") close.current();
      });
      win.focus();
    } catch {
      // Not the app's own origin after all: the Close button still works.
    }
  };

  return (
    <Modal onClose={onClose} className="flex w-full max-w-2xl flex-col gap-3 p-5">
      <h2 className="text-sm font-semibold text-text">Dino game</h2>
      <p className="text-xs text-muted">Space or Up to jump, Down to duck. Esc closes the game.</p>
      {/* tabIndex makes it the Modal's first stop, so focus lands in the game. */}
      <iframe
        ref={frame}
        title="Dino game"
        src={RUNNER_SRC}
        tabIndex={0}
        onLoad={onLoad}
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

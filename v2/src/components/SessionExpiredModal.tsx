// Shown when a request came back 401 after silent renewal gave up -
// typically the first fetch after days in hibernate. One app-level prompt
// with the fix on it, instead of a "Not authorized" error on every screen.
//
// "Not now" is allowed on purpose: cached data is still readable, so the
// user can finish looking at what they have; the next failed fetch raises
// the prompt again.
import { KeyRound } from "lucide-react";
import { Button } from "./ui/button";
import { Modal } from "./ui/modal";

export default function SessionExpiredModal({
  signingIn,
  onSignIn,
  onDismiss,
}: {
  signingIn: boolean;
  onSignIn: () => void;
  onDismiss: () => void;
}) {
  return (
    <Modal onClose={onDismiss} className="flex w-full max-w-sm flex-col gap-4 p-5">
      <div className="flex shrink-0 items-center gap-2">
        <KeyRound size={16} className="text-accent" />
        <h2 className="text-sm font-semibold text-text">Session expired</h2>
      </div>

      <p className="text-xs leading-relaxed text-muted">
        Your Azure DevOps sign-in is no longer valid - this happens after the app has been idle
        for a while (a weekend in hibernate is enough). Anything already loaded stays readable,
        but fetching or saving needs a fresh sign-in.
      </p>

      <div className="flex shrink-0 items-center justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onDismiss} disabled={signingIn}>
          Not now
        </Button>
        <Button size="sm" onClick={onSignIn} disabled={signingIn}>
          <KeyRound size={14} aria-hidden />
          {signingIn ? "Waiting for Microsoft..." : "Sign in again"}
        </Button>
      </div>
    </Modal>
  );
}

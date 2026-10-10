// The person's Allow or Deny before the assistant replays a script marked
// must not save (the Rust side is autorun::replay_ask). Nothing opens,
// signs in or runs before Allow.
//
// Mounted once in App.tsx and listening for as long as the app runs. The
// window stays loaded while it is hidden in the tray, so a request that
// arrives then is held here and shown when the window is opened again. It
// is state, not a toast: it stays until it is answered or the request
// stops waiting (answered elsewhere, timed out, or the assistant gone).
import { useEffect, useRef, useState } from "react";
import { commands, events, type AutorunReplayRequest } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";
import { toast } from "../../lib/toast";

/** The prompt's words, exactly as the design gives them. */
export function replayRequestText(r: AutorunReplayRequest): string {
  return `The assistant wants to replay case ${r.case_id} (${r.title}) up to step ${r.step}. This script must not save; the guard stays on. Allow?`;
}

/** The one line added while the assistant's discovery holds the browser:
 * Allow ends it first (the Rust side is `end_discovery_for_replay`). */
export const ENDS_DISCOVERY_LINE = "This ends the assistant's discovery first; what it mapped is kept.";

/** The extra line this request shows, if any. */
export function replayRequestDiscoveryLine(r: AutorunReplayRequest): string | null {
  return r.ends_discovery ? ENDS_DISCOVERY_LINE : null;
}

export default function ReplayRequestModal() {
  const [request, setRequest] = useState<AutorunReplayRequest | null>(null);
  const [answering, setAnswering] = useState(false);
  // Read by every way out, not only the buttons: Escape or a backdrop click
  // just after Allow must not send a second answer (a Deny) behind it.
  const inFlight = useRef(false);

  useEffect(() => {
    const asked = events.autorunReplayRequest.listen((e) => setRequest(e.payload));
    const ended = events.autorunReplayRequestEnded.listen((e) =>
      setRequest((cur) => (cur && cur.id === e.payload.id ? null : cur)),
    );
    return () => {
      for (const un of [asked, ended]) un.then((f) => f()).catch(() => {});
    };
  }, []);

  if (!request) return null;

  const answer = async (allow: boolean) => {
    if (inFlight.current) return;
    inFlight.current = true;
    setAnswering(true);
    const r = await commands.autoRunAnswerReplayRequest(request.id, allow);
    inFlight.current = false;
    setAnswering(false);
    // Closed whatever came back: a request that is no longer waiting has
    // nothing left to answer.
    setRequest((cur) => (cur && cur.id === request.id ? null : cur));
    if (r.status === "error") toast.warning(r.error);
  };

  return (
    <Modal
      onClose={() => void answer(false)}
      labelledBy="replay-request-title"
      className="flex w-full max-w-sm flex-col gap-4 p-5"
    >
      <h2 id="replay-request-title" className="text-sm font-semibold text-text">
        Replay request
      </h2>
      <p className="text-xs leading-relaxed text-muted">{replayRequestText(request)}</p>
      {replayRequestDiscoveryLine(request) && (
        <p className="text-xs leading-relaxed text-muted">{replayRequestDiscoveryLine(request)}</p>
      )}
      <div className="flex shrink-0 items-center justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={() => void answer(false)} disabled={answering}>
          <IconCancel aria-hidden />
          Deny
        </Button>
        <Button size="sm" onClick={() => void answer(true)} disabled={answering}>
          <IconConfirm aria-hidden />
          Allow
        </Button>
      </div>
    </Modal>
  );
}

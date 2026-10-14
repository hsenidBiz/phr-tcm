// The pause when the company network stops answering (an unattended run):
// the run waits by itself and carries on once the network is back, or gives
// up after a time limit. The panel says so, counts the wait, and offers Stop.
// It never shows an address: the event does not carry one.

import { useEffect, useState } from "react";
import { Button } from "../../components/ui/button";
import { IconStop } from "../../lib/actionIcons";

/** The time waited as m:ss. */
export function waitedLabel(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const s = total % 60;
  return `${Math.floor(total / 60)}:${s < 10 ? "0" : ""}${s}`;
}

export default function NetworkWaitPanel({
  giveUpAfterS,
  since,
  onStop,
}: {
  /** Seconds after which the run gives up. */
  giveUpAfterS: number;
  /** When the wait began (ms since the epoch); the panel's own mount time
   * when the caller does not know. */
  since?: number;
  /** Ends the run here. The button waits while it is on its way. */
  onStop: () => void | boolean | Promise<void | boolean>;
}) {
  const [began] = useState(() => since ?? Date.now());
  const [now, setNow] = useState(() => Date.now());
  const [stopping, setStopping] = useState(false);
  /** The stop went through: the button stays down until the wait's own end
   * removes this panel. */
  const [stopped, setStopped] = useState(false);
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  const left = giveUpAfterS - Math.floor((now - began) / 1000);
  const minutes = Math.max(1, Math.ceil(left / 60));
  const stop = async () => {
    setStopping(true);
    try {
      if ((await onStop()) === true) setStopped(true);
    } finally {
      setStopping(false);
    }
  };
  return (
    <section
      aria-label="Paused - the company network is unreachable"
      className="space-y-2 rounded-md border border-warning/40 bg-surface-2 p-3 text-xs"
    >
      <h3 className="text-sm font-semibold text-warning">Paused - the company network is unreachable</h3>
      <p role="status" className="text-muted">
        Reconnect the VPN; the run carries on by itself. Gives up in {minutes} min.
      </p>
      <p className="text-text">Waited {waitedLabel(now - began)}</p>
      <div className="flex justify-end">
        <Button
          size="sm"
          variant="outline"
          aria-label="Stop the run while it waits for the network"
          disabled={stopping || stopped}
          onClick={() => void stop()}
        >
          <IconStop aria-hidden />
          Stop
        </Button>
      </div>
    </section>
  );
}

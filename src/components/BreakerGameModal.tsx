import { isValidBest, loadBreakerBest, recordBreakerBest } from "../lib/breakerBest";
import GameFrameModal from "./GameFrameModal";

/** The bundled brick game, written for the app (no third-party code).
 * Settings' optional extras open it. */
export const BREAKER_SRC = "/vendor/breaker/index.html";

/** A `breaker-score` message whose best is a real score, or null. */
function scoreOf(data: unknown): number | null {
  if (!data || typeof data !== "object") return null;
  const d = data as { type?: unknown; best?: unknown };
  return d.type === "breaker-score" && isValidBest(d.best) ? d.best : null;
}

/**
 * The game in a sandboxed frame (see GameFrameModal). The frame has no
 * storage, so the best score lives app side: the frame posts
 * `{type: "breaker-score", best}` when a game ends and the higher score is
 * kept (lib/breakerBest.ts); each time the frame loads it is sent
 * `{type: "breaker-best", best}` to show.
 */
export default function BreakerGameModal({ onClose }: { onClose: () => void }) {
  return (
    <GameFrameModal
      title="Brick game"
      hint="Move with the mouse or Left and Right, Space or click to launch, P to pause. Catch what falls from a brick for a boost. Esc closes the game."
      src={BREAKER_SRC}
      escapeType="breaker-escape"
      onClose={onClose}
      frameClassName="h-96"
      onFrameMessage={(data) => {
        const best = scoreOf(data);
        if (best !== null) recordBreakerBest(best);
      }}
      onFrameLoad={(win) => {
        // An opaque-origin frame: "*" is the only target that reaches it.
        win.postMessage({ type: "breaker-best", best: loadBreakerBest() }, "*");
      }}
    />
  );
}

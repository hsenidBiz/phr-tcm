import GameFrameModal from "./GameFrameModal";

/** The bundled runner game (Chrome's offline T-Rex game) - see
 * public/vendor/runner/SOURCE.txt for where it came from and how it was
 * vetted. Settings' optional extras open it. */
export const RUNNER_SRC = "/vendor/runner/index.html";

/** The game in a sandboxed frame - see GameFrameModal for why sandboxed,
 * and how `escape.js` (bundled alongside the game, ours - see SOURCE.txt)
 * closes it. */
export default function RunnerGameModal({ onClose }: { onClose: () => void }) {
  return (
    <GameFrameModal
      title="Dino game"
      hint="Space or Up to jump, Down to duck. Esc closes the game."
      src={RUNNER_SRC}
      escapeType="runner-escape"
      onClose={onClose}
    />
  );
}

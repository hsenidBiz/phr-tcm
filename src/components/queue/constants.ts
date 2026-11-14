export const HOLD_REFUSAL =
  "Some cases from the last upload have an unknown outcome - check them before uploading again.";

/** Rust's `PLAIN_STEPS_REFUSAL` (commands/queue.rs), word for word: the
 * upload stopped before sending anything because the current steps of its
 * updates could not be read. Pinned on both sides by tests. */
export const PLAIN_STEPS_REFUSAL =
  "The current steps of the cases this upload updates could not be read, so nothing was sent. " +
  "If you upload anyway, their steps are written as plain text and any formatting or pictures in them are lost.";

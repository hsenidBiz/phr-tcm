// Rust's refusals of a copy, matched by what they say (their wording is
// pinned by a Rust test, `the_sentences_the_webview_matches_by_their_beginning_still_begin_so`)
// and put in words that fit a copy dialog.

/** Rust asked the person to agree to replace something. */
export function isNeedsConfirm(message: string): boolean {
  return message.includes("confirm to replace it");
}

/** A refusal as the copy dialogs show it. */
export function copyRefusalText(message: string): string {
  if (message.startsWith("the environment was switched")) {
    return "The environment changed while this dialog was open - open it again.";
  }
  if (message.startsWith("the environment cannot be switched")) {
    return "Nothing can be copied while something is open or running in Auto Run - finish or close it first.";
  }
  return message;
}

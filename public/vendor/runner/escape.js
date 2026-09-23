// Ours, not vendored - unlike every other file in this folder (see
// SOURCE.txt), this one was written for the app, not lifted from upstream.
//
// The game frame runs sandboxed (sandbox="allow-scripts", no
// allow-same-origin), so it has no access to the parent window beyond
// postMessage - not even to read its own document from outside. This is
// the frame's only way to ask to be closed. It does nothing else: no
// listeners beyond this one, no other message, no other target.
addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    parent.postMessage({ type: "runner-escape" }, "*");
  }
});

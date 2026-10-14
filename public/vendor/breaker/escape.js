// Written for the app. The frame runs sandboxed (sandbox="allow-scripts",
// no allow-same-origin), so a key pressed inside it never reaches the app:
// this relays Escape out as a message, which is what closes the modal. It
// does nothing else: no other listener, no other message, no other target.
addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    parent.postMessage({ type: "breaker-escape" }, "*");
  }
});

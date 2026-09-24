// The loading screen: `#splash` in index.html, painted before any script
// runs. It covers the launch until the app has something real to show -
// for the main window, the moment it knows whether someone is signed in
// (App.tsx); for the runner window, its first render (main.tsx). Taken away
// only once, however many callers ask.

const FADE_MS = 250;

let gone = false;

export function hideSplash(): void {
  if (gone) return;
  const splash = document.getElementById("splash");
  if (!splash) return;
  gone = true;
  splash.style.transition = `opacity ${FADE_MS}ms ease-out`;
  splash.style.opacity = "0";
  setTimeout(() => splash.remove(), FADE_MS + 50);
}

/** Tests only: forget that the loading screen was taken away. */
export function resetSplashForTests(): void {
  gone = false;
}

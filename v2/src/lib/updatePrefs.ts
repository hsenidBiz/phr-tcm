// The Settings switch "Only check Azure DevOps for updates". Read by every
// caller of checkUpdate / applyUpdate and passed to the backend on each
// call - the Rust side keeps no copy, so flipping it takes effect on the
// next check with nothing to sync. localStorage, same shape as
// `sidebarState.ts`: a storage write does not notify the same window, so
// the listener set does.
export const GITHUB_OFF_KEY = "tcm-v2-updates-github-off";

const listeners = new Set<() => void>();

export function githubOffSnapshot(): boolean {
  try {
    return localStorage.getItem(GITHUB_OFF_KEY) === "1";
  } catch {
    return false;
  }
}

export function setGithubOff(on: boolean): void {
  try {
    if (on) localStorage.setItem(GITHUB_OFF_KEY, "1");
    else localStorage.removeItem(GITHUB_OFF_KEY);
  } catch {
    // Storage unavailable: the switch is simply not remembered.
  }
  for (const l of listeners) l();
}

export function subscribeGithubOff(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

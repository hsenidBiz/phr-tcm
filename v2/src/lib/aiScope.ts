// Where the AI tools register: the working repository (the normal mode) or
// the whole machine. Machine-wide is opt-in from Settings - it is the
// pre-per-repo behaviour, kept for people who do not work from a
// repository - and the AI Bridge tab then offers the choice explicitly.
// Both values are observable so the tab and the gate react at once.

const ALLOW_KEY = "tcm-v2-ai-global-allowed";
const SCOPE_KEY = "tcm-v2-ai-scope";

export type RegistrationScope = "project" | "global";

export function loadGlobalAllowed(): boolean {
  try {
    return localStorage.getItem(ALLOW_KEY) === "on";
  } catch {
    return false;
  }
}

const listeners = new Set<() => void>();
const notify = () => {
  for (const l of listeners) l();
};

export function saveGlobalAllowed(on: boolean): void {
  try {
    if (on) localStorage.setItem(ALLOW_KEY, "on");
    else localStorage.removeItem(ALLOW_KEY);
  } catch {
    // storage unavailable -> nothing is remembered
  }
  notify();
}

export function loadScope(): RegistrationScope {
  try {
    return localStorage.getItem(SCOPE_KEY) === "global" ? "global" : "project";
  } catch {
    return "project";
  }
}

export function saveScope(scope: RegistrationScope): void {
  try {
    if (scope === "global") localStorage.setItem(SCOPE_KEY, "global");
    else localStorage.removeItem(SCOPE_KEY);
  } catch {
    // storage unavailable -> nothing is remembered
  }
  notify();
}

/** One subscription covers both values - they change from the same two
 * places (Settings, the AI Bridge card) and are read together. */
export function subscribeAiScope(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export function globalAllowedSnapshot(): boolean {
  return loadGlobalAllowed();
}

export function scopeSnapshot(): RegistrationScope {
  return loadScope();
}

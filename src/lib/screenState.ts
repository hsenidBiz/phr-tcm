/**
 * State that outlives a screen's mount.
 *
 * App.tsx mounts only the active tab, so a tab change unmounts the screen
 * and every `useState` in it starts again. That threw away a half-written
 * Manual Entry case and the search, filter and selection of View, Update
 * and Run Tests each time someone looked at another tab.
 *
 * `useScreenState` is `useState` with a key: the value is kept in a
 * module-scope map, so the next mount with the same key gets it back.
 * - Session only by default: nothing reaches disk, and a restart starts
 *   clean (the right default for search text and selections).
 * - `persist: true` also writes the value to localStorage under
 *   `tcm-v2-screen:<key>`, for work a person typed and would hate to lose
 *   (the Manual Entry draft). A failed write is not fatal: the value stays
 *   in memory for this session.
 * - A `null` key is plain `useState`: nothing kept, nothing written. Pass
 *   it while the guided tour runs, so sample data never outlives the tour
 *   and a real draft is never shown inside it.
 *
 * Keys must carry every scope the value depends on (org, PBI, project...):
 * a value saved for one PBI must never appear under another.
 */
import { useCallback, useEffect, useRef, useState, useSyncExternalStore, type Dispatch, type SetStateAction } from "react";
import { subscribeTour, tourRunningSnapshot } from "../tour/tourState";

const STORE_PREFIX = "tcm-v2-screen:";
const memory = new Map<string, unknown>();

function readPersisted<T>(key: string): T | undefined {
  try {
    const raw = localStorage.getItem(STORE_PREFIX + key);
    return raw == null ? undefined : (JSON.parse(raw) as T);
  } catch {
    return undefined;
  }
}

function writePersisted(key: string, value: unknown, initial: unknown): void {
  try {
    // A value back at its starting point removes the key, so an empty form
    // leaves nothing on disk.
    if (JSON.stringify(value) === JSON.stringify(initial)) {
      localStorage.removeItem(STORE_PREFIX + key);
    } else {
      localStorage.setItem(STORE_PREFIX + key, JSON.stringify(value));
    }
  } catch {
    // storage full or unavailable - the value stays in memory
  }
}

function resolve<T>(initial: T | (() => T)): T {
  return typeof initial === "function" ? (initial as () => T)() : initial;
}

function lookup<T>(key: string | null, initial: T | (() => T), persist: boolean): T {
  if (key == null) return resolve(initial);
  if (memory.has(key)) return memory.get(key) as T;
  if (persist) {
    const saved = readPersisted<T>(key);
    if (saved !== undefined) {
      memory.set(key, saved);
      return saved;
    }
  }
  return resolve(initial);
}

export function useScreenState<T>(
  key: string | null,
  initial: T | (() => T),
  opts: { persist?: boolean } = {},
): [T, Dispatch<SetStateAction<T>>] {
  const persist = opts.persist ?? false;
  const [state, setState] = useState<{ key: string | null; value: T }>(() => ({
    key,
    value: lookup(key, initial, persist),
  }));
  // The initial value of the CURRENT key, kept so a persisted value equal
  // to it can be removed from disk instead of stored.
  const initialRef = useRef(initial);
  initialRef.current = initial;

  // A key change (another PBI, the tour starting or ending) loads that
  // key's value. Until this runs the state still carries the old key, and
  // the setter below writes against the key the value belongs to, so one
  // scope's value is never filed under another.
  let current = state;
  if (state.key !== key) {
    current = { key, value: lookup(key, initial, persist) };
    setState(current);
  }

  const setValue = useCallback<Dispatch<SetStateAction<T>>>(
    (update) => {
      setState((prev) => {
        const value = typeof update === "function" ? (update as (p: T) => T)(prev.value) : update;
        if (prev.key != null) {
          memory.set(prev.key, value);
          if (persist) writePersisted(prev.key, value, resolve(initialRef.current));
        }
        return { key: prev.key, value };
      });
    },
    [persist],
  );

  return [current.value, setValue];
}

/** Forget every kept value whose key starts with `prefix` (memory and disk). */
export function forgetScreenState(prefix: string): void {
  for (const k of [...memory.keys()]) if (k.startsWith(prefix)) memory.delete(k);
  try {
    const stale: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k && k.startsWith(STORE_PREFIX + prefix)) stale.push(k);
    }
    for (const k of stale) localStorage.removeItem(k);
  } catch {
    // storage unavailable
  }
}

/** Test hook: drop all session-only values. */
export function resetScreenStateForTests(): void {
  memory.clear();
}

/** Keeps a Set in screen state. Sets do not survive JSON, so this is for
 * session-only (not persisted) values such as row selections. */
export function useScreenSet<T>(key: string | null): [Set<T>, Dispatch<SetStateAction<Set<T>>>] {
  return useScreenState<Set<T>>(key, () => new Set<T>());
}

/**
 * Key maker for one screen's kept values: `field => "<scope>:<field>"`, or
 * `null` keys (plain useState) while the tour runs or when there is no
 * scope yet (no PBI picked).
 */
export function useScreenKeys(scope: string | null): (field: string) => string | null {
  const tour = useSyncExternalStore(subscribeTour, tourRunningSnapshot);
  return (field: string) => (tour || scope == null ? null : `${scope}:${field}`);
}

/**
 * Drops ids from a kept selection that the loaded list no longer has (a
 * case removed or moved while the screen was away). Runs only once the list
 * is loaded, so a selection is not wiped while data is still on its way.
 */
export function usePruneSelection(
  loaded: readonly number[] | undefined,
  setSelected: Dispatch<SetStateAction<Set<number>>>,
): void {
  useEffect(() => {
    if (!loaded) return;
    const present = new Set(loaded);
    setSelected((prev) => {
      if ([...prev].every((id) => present.has(id))) return prev;
      return new Set([...prev].filter((id) => present.has(id)));
    });
  }, [loaded, setSelected]);
}

import { useCallback, useState } from "react";

/** A Set<string> backed by localStorage, so collapsed Group-by-Title groups
 * survive across sessions. Returns the set, a toggle for one name, and a
 * bulk add for the sticky "Collapse groups" button - callers that only
 * destructure the first two are unaffected. */
export function usePersistedStringSet(
  key: string,
): [Set<string>, (name: string) => void, (names: string[]) => void] {
  const [set, setSet] = useState<Set<string>>(() => {
    try {
      const raw = localStorage.getItem(key);
      return raw ? new Set(JSON.parse(raw) as string[]) : new Set();
    } catch {
      return new Set();
    }
  });

  const toggle = useCallback(
    (name: string) => {
      setSet((s) => {
        const next = new Set(s);
        if (next.has(name)) next.delete(name);
        else next.add(name);
        try {
          localStorage.setItem(key, JSON.stringify([...next]));
        } catch {
          // storage unavailable -> session-only
        }
        return next;
      });
    },
    [key],
  );

  const addAll = useCallback(
    (names: string[]) => {
      setSet((s) => {
        const next = new Set(s);
        for (const n of names) next.add(n);
        try {
          localStorage.setItem(key, JSON.stringify([...next]));
        } catch {
          // storage unavailable -> session-only
        }
        return next;
      });
    },
    [key],
  );

  return [set, toggle, addAll];
}

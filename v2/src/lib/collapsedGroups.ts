import { useCallback, useState } from "react";

/** A Set<string> backed by localStorage, so collapsed Group-by-Title groups
 * survive across sessions. Returns the set plus a toggle for one name. */
export function usePersistedStringSet(key: string): [Set<string>, (name: string) => void] {
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

  return [set, toggle];
}

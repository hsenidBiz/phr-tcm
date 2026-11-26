// December snow in every theme but the two that have their own (Halloween
// has none; the Christmas theme draws its own in ChristmasDecor). On by
// default, off with the Settings switch (a per-machine display pref, kept
// like the theme in localStorage). A machine with the hidden Extras switch
// on sees it at any time of year. Never in a docs capture, never under
// reduced motion.
import { useEffect, useState, useSyncExternalStore } from "react";
import { snowSeason } from "./season";

export const SNOW_OFF_KEY = "tcm-v2-snow-off";

const listeners = new Set<() => void>();

export function getSnowOff(): boolean {
  try {
    return localStorage.getItem(SNOW_OFF_KEY) === "1";
  } catch {
    return false;
  }
}

export function setSnowOff(off: boolean): void {
  try {
    if (off) localStorage.setItem(SNOW_OFF_KEY, "1");
    else localStorage.removeItem(SNOW_OFF_KEY);
  } catch {
    // storage unavailable: session-only
  }
  for (const l of [...listeners]) l();
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useSnowOff(): boolean {
  return useSyncExternalStore(subscribe, getSnowOff);
}

/** Whether snow can fall at all right now: the window is open (or this
 * machine is unlocked) and this is not a docs capture. Gates the switch. */
export function snowCanFall(o: { now: number; unlocked: boolean; capture: boolean }): boolean {
  return !o.capture && (snowSeason(o.now) || o.unlocked);
}

/** Whether the plain snow layer shows. `theme` is the document's theme id. */
export function snowShows(o: {
  now: number;
  theme: string;
  snowOff: boolean;
  unlocked: boolean;
  capture: boolean;
  reducedMotion: boolean;
}): boolean {
  return snowCanFall(o) && !o.reducedMotion && !o.snowOff && o.theme !== "halloween" && o.theme !== "christmas";
}

const MINUTE = 60_000;

/** The clock, re-read about every minute, so the snow starts at midnight on
 * 1 December and stops on 1 January without a restart. */
export function useNow(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), MINUTE);
    return () => clearInterval(t);
  }, []);
  return now;
}

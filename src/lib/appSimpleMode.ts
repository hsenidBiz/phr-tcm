// App Simple mode as the webview sees it. Rust owns the setting
// (AppSettings.app_simple_mode); this mirrors it in a module-scope store so a
// plain function (`sectionShown`) and a hook can both read it. The App shell
// publishes the app-settings query into it (`usePublishAppSimpleMode`).
//
// While the settings are still loading the store reads as off (show
// everything), the same rule the Auto Run mode follows. In the documentation
// screenshots the sample app answers the setting in memory (dev/demo.ts), off
// until the one shot that documents the switch clicks it, so the guide keeps
// showing every screen.
import { useQuery } from "@tanstack/react-query";
import { useEffect, useSyncExternalStore } from "react";
import { commands } from "../bindings";

/** The key the app's settings are cached under (Settings, Background, ...). */
const APP_SETTINGS_KEY = ["app-settings"] as const;

let on = false;
let loaded = false;
const listeners = new Set<() => void>();

function notify(): void {
  for (const l of [...listeners]) l();
}

function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Whether Simple mode is on right now. False while loading. */
export function appSimpleModeSnapshot(): boolean {
  return loaded && on;
}

function loadedSnapshot(): boolean {
  return loaded;
}

/** Put what the settings say into the store. `settled` is true once the
 * settings query has answered, successfully or not: a failed read must not
 * leave the redirect waiting forever. */
export function publishAppSimpleMode(next: boolean, settled: boolean): void {
  if (next === on && settled === loaded) return;
  on = next;
  loaded = settled;
  notify();
}

export function useAppSimpleMode(): { on: boolean; loading: boolean } {
  const isOn = useSyncExternalStore(subscribe, appSimpleModeSnapshot);
  const isLoaded = useSyncExternalStore(subscribe, loadedSnapshot);
  return { on: isOn, loading: !isLoaded };
}

/** Mounted once in the app shell: follows the settings query into the store. */
export function usePublishAppSimpleMode(): void {
  const settings = useQuery({ queryKey: APP_SETTINGS_KEY, queryFn: () => commands.getAppSettings() });
  const next = settings.data?.app_simple_mode === true;
  const settled = settings.data !== undefined || settings.isError;
  useEffect(() => {
    publishAppSimpleMode(next, settled);
  }, [next, settled]);
}

/** Tests only: set Simple mode and mark the store loaded. */
export function setAppSimpleModeForTest(next: boolean): void {
  publishAppSimpleMode(next, true);
}

/** Tests only: back to off and still loading. */
export function resetAppSimpleModeStore(): void {
  on = false;
  loaded = false;
  switchFailures = [];
  notify();
}

// The per-environment switches Simple mode could not turn on (the webview's
// half of "turning it on"). Settings reads this beside the modules Rust
// could not switch on.
let switchFailures: string[] = [];
const failureListeners = new Set<() => void>();

function subscribeFailures(cb: () => void): () => void {
  failureListeners.add(cb);
  return () => {
    failureListeners.delete(cb);
  };
}

export function publishSimpleSwitchFailures(names: string[]): void {
  if (names.length === switchFailures.length && names.every((n, i) => n === switchFailures[i])) return;
  switchFailures = names;
  for (const l of [...failureListeners]) l();
}

/** Names of the switches the last Simple mode switch-on could not set. */
export function useSimpleSwitchFailures(): string[] {
  return useSyncExternalStore(subscribeFailures, () => switchFailures);
}

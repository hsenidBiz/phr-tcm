import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useRef, useState } from "react";
import { isCaptureMode } from "../dev/capture";
import { markChangelogSeen, SHOW_CHANGELOG_EVENT, updatedFrom } from "../lib/changelogSeen";
import type { ChangelogEntry } from "../lib/changelog";
import { DEV_TOOLS } from "./lazyScreens";

/**
 * Post-update "What's new": once per version change, after sign-in (so it
 * never covers the sign-in screen). Fresh installs record the version
 * silently - see `updatedFrom` in lib/changelogSeen.ts for the rules. The
 * entries load only when this launch follows an update.
 */
export function useChangelog(signedIn: boolean) {
  const [changelog, setChangelog] = useState<ChangelogEntry[] | null>(null);
  const shownChangelogRef = useRef(false);
  useEffect(() => {
    // Capture mode: a "what's new" modal must never cover a shot.
    if (!signedIn || shownChangelogRef.current || isCaptureMode()) return;
    shownChangelogRef.current = true;
    getVersion()
      .then(async (v) => {
        const seen = updatedFrom(v);
        if (!seen) return;
        const { entriesSince } = await import("../lib/changelog");
        const pending = entriesSince(seen, v);
        if (pending.length > 0) setChangelog(pending);
      })
      .catch(() => {
        // version unavailable (tests) - skip quietly
      });
  }, [signedIn]);

  const dismissChangelog = () => {
    getVersion()
      .then(markChangelogSeen)
      .catch(() => {});
    setChangelog(null);
  };
  // Dev-only preview trigger (DevPanel -> window event). Two entries so the
  // multi-version stacking is visible. Compile-time eliminated in releases.
  useEffect(() => {
    if (!DEV_TOOLS) return;
    const fire = () => {
      void import("../lib/changelog").then((m) => setChangelog(m.CHANGELOG.slice(0, 2)));
    };
    window.addEventListener(SHOW_CHANGELOG_EVENT, fire);
    return () => window.removeEventListener(SHOW_CHANGELOG_EVENT, fire);
  }, []);

  return { changelog, dismissChangelog };
}

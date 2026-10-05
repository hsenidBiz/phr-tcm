/**
 * The small part of the changelog the app needs at startup: comparing
 * versions, telling a beta from a release, and remembering which version's
 * "What's new" was last seen. It holds none of the entries, so App can
 * decide whether an update happened without loading them; the entries
 * themselves (`lib/changelog.ts`) load only when there is something to show.
 * `lib/changelog.ts` re-exports everything here, so imports from there keep
 * working.
 */

/** Semver compare: -1 / 0 / 1 for a < b / a == b / a > b. A prerelease
 * (`1.26.0-beta.2`) sorts below its release (`1.26.0`), and its numeric
 * parts compare as numbers (`beta.10` > `beta.9`). Anything that is not
 * X.Y.Z (e.g. "dev") reads as 0.0.0, which keeps What's new shut in dev. */
export function compareVersions(a: string, b: string): number {
  const parse = (v: string) => {
    const m = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/.exec(v.trim());
    if (!m) return { core: [0, 0, 0], pre: [] as string[] };
    return { core: [Number(m[1]), Number(m[2]), Number(m[3])], pre: m[4] ? m[4].split(".") : [] };
  };
  const pa = parse(a);
  const pb = parse(b);
  for (let i = 0; i < 3; i++) {
    if (pa.core[i] !== pb.core[i]) return pa.core[i] < pb.core[i] ? -1 : 1;
  }
  // No prerelease outranks any prerelease of the same X.Y.Z.
  if (!pa.pre.length || !pb.pre.length) return pa.pre.length === pb.pre.length ? 0 : pa.pre.length ? -1 : 1;
  for (let i = 0; i < Math.max(pa.pre.length, pb.pre.length); i++) {
    const x = pa.pre[i];
    const y = pb.pre[i];
    if (x === undefined) return -1;
    if (y === undefined) return 1;
    const nx = /^\d+$/.test(x) ? Number(x) : null;
    const ny = /^\d+$/.test(y) ? Number(y) : null;
    if (nx !== null && ny !== null) {
      if (nx !== ny) return nx < ny ? -1 : 1;
    } else if (nx !== null || ny !== null) {
      return nx !== null ? -1 : 1;
    } else if (x !== y) {
      return x < y ? -1 : 1;
    }
  }
  return 0;
}

/** Whether `v` is a beta build's version (`X.Y.Z-beta.N`). */
export function isBetaVersion(v: string): boolean {
  return /^\d+\.\d+\.\d+-beta\.\d+$/.test(v.trim());
}

/** Dev-only trigger: the DevPanel dispatches this window event to preview
 * the post-update modal; App's DEV-gated listener responds. Lives here (not
 * in dev/) so App can import it without statically pulling the dev module. */
export const SHOW_CHANGELOG_EVENT = "tcm-v2-dev-show-changelog";

const SEEN_KEY = "tcm-v2-changelog-seen";

/** Whether this launch follows an update, and from which version:
 * - fresh install (nothing stored): remember the version and answer null,
 *   because installing is not updating;
 * - stored version older than `current`: answer the stored version;
 * - otherwise (same version, a downgrade, a dev build, or no storage): null. */
export function updatedFrom(current: string): string | null {
  let seen: string | null = null;
  try {
    seen = localStorage.getItem(SEEN_KEY);
  } catch {
    return null;
  }
  if (!seen) {
    markChangelogSeen(current);
    return null;
  }
  return compareVersions(current, seen) > 0 ? seen : null;
}

export function markChangelogSeen(version: string): void {
  try {
    localStorage.setItem(SEEN_KEY, version);
  } catch {
    // storage unavailable - the modal may show again next launch
  }
}

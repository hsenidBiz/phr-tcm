/**
 * In-app changelog. RELEASE RULE: every release adds its entry here (newest
 * first) BEFORE running release-v2.ps1 - the post-update "What's new" modal
 * only fires when an entry newer than the last-seen version exists, so a
 * release without an entry updates silently.
 */

export type ChangelogEntry = {
  version: string;
  date: string; // YYYY-MM-DD
  items: string[];
};

export const CHANGELOG: ChangelogEntry[] = [
  {
    version: "1.12.0",
    date: "2026-07-26",
    items: [
      "Pull Requests: switch between active and completed pull requests on a repository.",
      "Pull Requests: expanding a PR now shows its latest pipeline run - whether it passed, and if it failed, exactly which stage, job and step broke.",
      "New pipeline history view: a timeline of every run for the PR, with stages, jobs and steps, a search box, a failures-only filter, and the environments each build was deployed to.",
      "Click any step to read its log, exactly as Azure DevOps shows it - including live output while a build is still running.",
      "Settings: choose how hard the app hits Azure DevOps (Full speed / Balanced / Gentle). Azure DevOps limits requests per user, so easing off helps when you are working in the browser at the same time.",
      "Settings: the app now keeps its own log - view it beside the changelog, copy it, or open the log folder when reporting a problem.",
    ],
  },
  {
    version: "1.11.2",
    date: "2026-07-25",
    items: [
      "AI Bridge: a Rescan button re-checks which AI tools are installed, so a newly installed one shows up without restarting the app.",
      "AI Bridge: the tool list now says it is scanning instead of briefly claiming no tools were found.",
    ],
  },
  {
    version: "1.11.1",
    date: "2026-07-25",
    items: [
      "AI Bridge: an Unregister button beside each connected tool, so a registration can be removed from the app instead of by hand.",
      "AI Bridge: the six tools AI assistants can use are now listed with what each one does.",
      "Settings: the changelog moves into its own column on wide windows, and stacks as before on narrow ones.",
      "Board: hiding or showing a column now fades its cards out before it collapses (and in after it widens), so card text no longer squishes mid-animation.",
    ],
  },
  {
    version: "1.11.0",
    date: "2026-07-25",
    items: [
      "New AI Bridge tab (Ctrl+7): connect AI assistants to this app over MCP. Installed tools (Claude Code, Claude Desktop, VS Code Copilot, Cursor, Windsurf) are detected and registered with one click; a copyable command and config snippet cover everything else.",
      "AI assistants get six read-only tools: the writing guide with your org's live module list, real example test cases from a PBI, draft validation through the app's real importer, PBI search, and wiki search with full page reads for finding documentation.",
      "Nothing extra to install - the app itself acts as the MCP server. AI can only read; it can never create, change, or delete anything in Azure DevOps through this bridge.",
      "Pull Requests: repo names now show as accent pills, matching the board's PR chips.",
      "Toasts no longer highlight their text when you try to drag them away.",
    ],
  },
  {
    version: "1.10.3",
    date: "2026-07-24",
    items: [
      "Work Manager: new \"New Work Item\" tab - a full creation form (type, title, assignee, area, iteration, priority, tags, description, optional parent PBI) replaces the board's quick-create row.",
      "Runner: screen recording like Azure DevOps's runner - Record, pick a screen, and the video attaches to the result.",
      "Runner: the test case's preconditions now show above the steps, and the pin (always-on-top) toggle is remembered between runs.",
      "Board: each column has its own eye to hide it (at least one always stays visible), replacing the Hide Done checkbox.",
      "Board: when a move is blocked by required fields, the app names them, opens the item, and highlights exactly what to fill.",
      "Iteration pickers show each sprint's dates, like Azure DevOps.",
      "Azure DevOps errors now show the server's real explanation instead of a bare HTTP status.",
    ],
  },
  {
    version: "1.10.2",
    date: "2026-07-23",
    items: [
      "Fixed: your chosen theme (OLED, Midnight, ...) no longer reverts to Slate when switching screens.",
    ],
  },
  {
    version: "1.10.1",
    date: "2026-07-22",
    items: [
      "Board: \"This sprint\" now works on area boards - it reads that team's own sprint (each team has its own here), and falls back to the previous sprint when a new one hasn't been created yet.",
      "Board: hiding Done no longer leaves a long empty scroll, and area boards gained a filter by assignee.",
      "Board: retired teams parked under \"Scrum Archive\" no longer clutter the area picker.",
      "Refreshed styling on pull-request descriptions, empty states, and screenshot previews (now open fullscreen with zoom).",
      "Dialogs now always cover the screen and stay centered, even with a long list behind them.",
    ],
  },
  {
    version: "1.10.0",
    date: "2026-07-21",
    items: [
      "What's new popup after updates (you're looking at it) plus a full changelog history in Settings.",
      "Queued test cases can carry comments - saved in the exported JSON and shown in the queue, never sent to Azure DevOps.",
      "If a PBI has no test plan when you submit, one is created automatically - and the app now tells you.",
      "\"Edit Test Cases\" is now \"Update Test Cases\".",
      "Pull Requests: descriptions render as rich text, expanded PRs list their linked work items, and your own PRs no longer appear twice.",
      "Work Manager screens now glide in when switching, like the rest of the app.",
      "Internal: release builds no longer bundle development demo data.",
    ],
  },
  {
    version: "1.9.0",
    date: "2026-07-20",
    items: [
      "Queued test cases can be edited in place - fix a title, steps, tags or module before submitting, without re-importing.",
    ],
  },
  {
    version: "1.8.0",
    date: "2026-07-18",
    items: [
      "Work Manager: new Pull Requests panel - PRs awaiting your review, your own PRs, and everything active on a chosen repository.",
      "Work Manager gets its own sidebar (Pull Requests and Board).",
      "Board cards show linked pull requests as repo-named chips.",
      "Board scope now uses Areas instead of the stale team list, and can scope to a single PBI.",
      "Stale items (untouched for a week) get a warning edge; a \"This sprint\" filter narrows the board to the current iteration.",
    ],
  },
  {
    version: "1.7.2",
    date: "2026-07-17",
    items: ["Internal restructuring of the backend modules - no visible changes."],
  },
  {
    version: "1.7.1",
    date: "2026-07-16",
    items: [
      "A calmer sign-in screen: animated title, subtle moving background, and a redrawn flask animation.",
      "Small motion polish across screens (counts, transitions, shimmer accents).",
    ],
  },
];

/** Numeric semver compare: -1 / 0 / 1 for a < b / a == b / a > b.
 * Non-numeric parts (e.g. "dev") compare as 0-padded numbers -> equal-ish,
 * which safely disables the modal in dev builds. */
export function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map((n) => parseInt(n, 10) || 0);
  const pb = b.split(".").map((n) => parseInt(n, 10) || 0);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return d < 0 ? -1 : 1;
  }
  return 0;
}

/** Entries strictly newer than `seen`, up to and including `current`. */
export function entriesSince(seen: string, current: string): ChangelogEntry[] {
  return CHANGELOG.filter(
    (e) => compareVersions(e.version, seen) > 0 && compareVersions(e.version, current) <= 0,
  );
}

/** Dev-only trigger: the DevPanel dispatches this window event to preview
 * the post-update modal; App's DEV-gated listener responds. Lives here (not
 * in dev/) so App can import it without statically pulling the dev module. */
export const SHOW_CHANGELOG_EVENT = "tcm-v2-dev-show-changelog";

const SEEN_KEY = "tcm-v2-changelog-seen";

/** What the post-update check should do for this launch:
 * - fresh install (nothing stored): remember the version, show nothing -
 *   installing is not updating;
 * - stored version older than current AND entries exist: show those entries;
 * - otherwise: nothing. */
export function pendingChangelog(current: string): ChangelogEntry[] {
  let seen: string | null = null;
  try {
    seen = localStorage.getItem(SEEN_KEY);
  } catch {
    return [];
  }
  if (!seen) {
    markChangelogSeen(current);
    return [];
  }
  if (compareVersions(current, seen) <= 0) return [];
  return entriesSince(seen, current);
}

export function markChangelogSeen(version: string): void {
  try {
    localStorage.setItem(SEEN_KEY, version);
  } catch {
    // storage unavailable - the modal may show again next launch
  }
}

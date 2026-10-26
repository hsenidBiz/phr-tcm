// The setup a run needs, beside the Test cases list: the environment runs
// go to and its database, the site address, the sign-in, the accounts, the
// areas, the test files and the save words. The search box for the case list
// sits at its top, on both tabs.
//
// Collapsed, it is one line per item with a status dot. Expanded, it is the
// full rows with their buttons, each opening its own dialog. The screen
// decides once, when the setup has loaded, whether it starts expanded (see
// `index.tsx`); after that only the person's toggle moves it.
//
// `useAutoRunSetup` reads everything the panel says, and whether a run has
// what it needs, so the screen and the panel never read the same thing twice. Readiness
// itself is `useAutoRunReadiness`'s, called once, here.

import { useQuery } from "@tanstack/react-query";
import { TriangleAlert } from "lucide-react";
import { useId, useMemo, useState, type ReactNode } from "react";
import { commands, type CaseScript } from "../../bindings";
import { Button } from "../../components/ui/button";
import { cn } from "../../lib/cn";
import { activeEnvironment, effectiveSite, useEnvironments } from "../../lib/environments";
import { unwrapStr } from "../../lib/ipc";
import {
  IconAccounts,
  IconComponents,
  IconDiscoveryMap,
  IconHideDetails,
  IconModulePaths,
  IconProtectedRecords,
  IconRecipe,
  IconRecord,
  IconRelease,
  IconSaveWords,
  IconShowDetails,
  IconSiteAddress,
  IconStop,
  IconTestFiles,
} from "../../lib/actionIcons";
import { useDiscoveryActive } from "../../lib/discoveryActive";
import { plural } from "../../lib/plural";
import AccountsDialog from "./AccountsDialog";
import AreasDialog from "./AreasDialog";
import CaseSearch from "./CaseSearch";
import ComponentsDialog, { componentsSummary, useComponents } from "./ComponentsDialog";
import DiscoveryDialog, { discoverySummary, useDiscoveryMap } from "./DiscoveryDialog";
import ProtectedRecordsDialog from "./ProtectedRecordsDialog";
import RecipeEditor from "./RecipeEditor";
import RecordSignInDialog from "./RecordSignInDialog";
import SaveWordsDialog, { BUILT_IN_SAVE_WORDS } from "./SaveWordsDialog";
import SiteAddressDialog from "./SiteAddressDialog";
import TestFilesDialog, { useTestFiles } from "./TestFilesDialog";
import { useAutoRunReadiness } from "./useAutoRunReadiness";

/** Everything the Setup panel reports, and what the screen needs to know
 * about it. All of it
 * reads small local files, and each read shares its query key with the
 * dialog that edits it, so a save there updates the panel here. `?? null`
 * because "nothing saved yet" is an answer, not a missing one. */
export function useAutoRunSetup({
  org,
  project,
  scripts,
}: {
  org: string;
  project: string;
  /** The listed cases' scripts, for the test files they upload. */
  scripts: (CaseScript | null | undefined)[];
}) {
  const setupReady = Boolean(org && project);
  const recipe = useQuery({
    queryKey: ["autorun-recipe", org, project],
    queryFn: async () => (await unwrapStr(commands.autoRunLoadRecipe(org, project))) ?? null,
    enabled: setupReady,
    retry: false,
  });
  const envs = useEnvironments();
  const accounts = useQuery({
    queryKey: ["autorun-accounts"],
    queryFn: async () => (await unwrapStr(commands.autoRunListAccounts())) ?? null,
    retry: false,
  });
  const nav = useQuery({
    queryKey: ["autorun-nav", org, project],
    queryFn: async () => (await unwrapStr(commands.autoRunLoadNav(org, project))) ?? null,
    enabled: setupReady,
    retry: false,
  });
  /** A count once its query has answered - `null` only while it has not
   * (pending) or could not (error). An answer of "no data" counts as none:
   * keyed on the answer rather than on the data, so the row can never sit
   * on "Loading…" after the query has already settled. */
  const accountCount = accounts.isSuccess ? (accounts.data?.length ?? 0) : null;
  // The documents this project's scripts upload. Shares its key with the
  // dialog, so adding or removing one there updates the row.
  const testFiles = useTestFiles(org, project);
  const testFileCount = testFiles.isSuccess ? (testFiles.data?.length ?? 0) : null;
  const areaCount = nav.isSuccess ? (nav.data?.modules.length ?? 0) : null;
  // What discovery has mapped. Shares its key with the Discovery dialog, so
  // Forget map there updates the row.
  const discoveryMap = useDiscoveryMap(org, project);
  const discoveryLine = discoverySummary(discoveryMap.data);
  // The project's components. Shares its key with the Components dialog,
  // so Remove there updates the row.
  const components = useComponents(org, project);
  const componentsLine = componentsSummary(components.data);

  const saved = recipe.data;
  // Where a run goes now: the active environment's address when it has one,
  // else the saved recipe's. With no saved recipe the built-in one signs in
  // at the environment's.
  const site = effectiveSite(envs.data, saved);
  const activeEnv = activeEnvironment(envs.data);
  /** The same address, as far as it is KNOWN: `undefined` while the
   * environments or the recipe it may fall back to have not answered, or
   * could not be read - an unreadable recipe may well hold an address, so
   * it never reads as "none". */
  const knownSiteUrl = ((): string | undefined => {
    if (envs.isPending) return undefined;
    const own = activeEnv?.start_url.trim();
    if (own) return own;
    if (setupReady && (recipe.isPending || recipe.isError)) return undefined;
    if (site.start_url) return site.start_url;
    return envs.isError ? undefined : "";
  })();
  const testFileNames = useMemo(
    () => (testFiles.isSuccess ? (testFiles.data ?? []).map((f) => f.name) : null),
    [testFiles.isSuccess, testFiles.data],
  );
  // No project, no sign-in: the recipe is a project's. A recipe that could
  // not be read is unknown, and its row says why.
  const signIn: "saved" | "builtin" | "none" | null = !setupReady
    ? "none"
    : recipe.isSuccess
      ? saved
        ? "saved"
        : "builtin"
      : null;
  const readiness = useAutoRunReadiness({
    siteUrl: knownSiteUrl,
    signIn,
    accountCount,
    areaCount,
    scripts,
    testFileNames,
  });

  /** Every essential read has answered, or failed. A failed read is shown
   * on its row and is never "missing" (see `useAutoRunReadiness`). */
  const essentialsSettled =
    !envs.isPending && !(setupReady && recipe.isPending) && !accounts.isPending;
  /** Reads that failed of the three a run cannot go without, each as the
   * sentence its row already says. They flag the panel. */
  const essentialUnreadable = [
    ...(envs.isError ? ["The environments could not be read"] : []),
    ...(setupReady && recipe.isError ? ["The saved recipe could not be read"] : []),
    ...(accounts.isError ? ["The accounts could not be read"] : []),
  ];

  /** The active environment's database. The same list, under the same
   * key, AI Bridge reads and edits. */
  const databases = useQuery({
    queryKey: ["db-databases"],
    queryFn: async () => (await commands.dbDatabases()) ?? [],
    retry: false,
  });
  const activeDb = databases.data?.find((d) => d.id === activeEnv?.db_id);

  return {
    setupReady,
    recipe,
    envs,
    accounts,
    nav,
    testFiles,
    discoveryMap,
    discoveryLine,
    components,
    componentsLine,
    databases,
    activeDb,
    accountCount,
    testFileCount,
    areaCount,
    site,
    activeEnv,
    knownSiteUrl,
    signIn,
    readiness,
    essentialsSettled,
    essentialUnreadable,
    /** Something a run cannot go without is missing, or could not be read,
     * or a script uploads a file the Test files folder does not hold. */
    attention:
      readiness.essentialMissing || essentialUnreadable.length > 0 || readiness.missingTestFiles.length > 0,
  };
}

export type AutoRunSetup = ReturnType<typeof useAutoRunSetup>;

/** One of the full Setup rows: what it is and its buttons on one line,
 * then where it stands, at the panel's full width below them. The panel is
 * a narrow column, so a value never shares its line with the buttons: an
 * address or a list of words gets the whole width to wrap in. A labelled
 * group, so a screen reader hears the row's name with its state and its
 * buttons. */
function SetupRow({
  label,
  status,
  state,
  children,
}: {
  label: string;
  /** The row's status, as the collapsed summary shows it. */
  status: Status;
  state: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div
      role="group"
      aria-label={label}
      className="space-y-1 border-t border-border/60 pt-3 first-of-type:border-t-0 first-of-type:pt-0"
    >
      {/* The label never gives up its width to the buttons: when they do not fit beside it, they
          wrap onto the next line instead of covering it. */}
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span className="mr-auto flex shrink-0 items-center gap-1.5 text-xs font-medium text-muted">
          <StatusDot status={status} />
          {label}
          {SPOKEN[status] && <span className="sr-only">{SPOKEN[status]}</span>}
        </span>
        <span className="flex max-w-full flex-wrap justify-end gap-2">{children}</span>
      </div>
      <div className="break-words text-sm text-text">{state}</div>
    </div>
  );
}

/** Ready, missing (a run cannot go without it), a warning (worth a look,
 * or could not be read), or quiet (loading, or nothing to say). */
type Status = "ready" | "missing" | "warning" | "quiet";

const DOT: Record<Status, string> = {
  ready: "bg-success",
  missing: "bg-danger",
  warning: "bg-warning",
  quiet: "bg-border-strong",
};

/** What a screen reader hears after the line, since the dot is a look. */
const SPOKEN: Record<Status, string> = {
  ready: ", ready",
  missing: ", missing",
  warning: ", needs a look",
  quiet: "",
};

/** The status dot, the same look in the summary and the full rows. A look
 * only: the status is spoken in words beside it. In the summary it sits in
 * a line-tall box of its own (`boxed`) so it stays beside a label's first
 * line when the value wraps. */
function StatusDot({ status, boxed = false }: { status: Status; boxed?: boolean }) {
  const dot = <span data-status-dot={status} className={cn("size-2 shrink-0 rounded-full", DOT[status])} />;
  return (
    <span aria-hidden className={boxed ? "flex h-5 shrink-0 items-center" : "inline-flex items-center"}>
      {dot}
    </span>
  );
}

/** A web address as text with a break opportunity after each `/`, `.`, `?`,
 * `&` and `=`, so a long one wraps at its own punctuation instead of in the
 * middle of a word. */
export function breakableUrl(url: string): ReactNode[] {
  return url.split(/(?<=[/.?&=])/).flatMap((part, i) => (i === 0 ? [part] : [<wbr key={i} />, part]));
}

/** How many names a summary line spells out before "and N more". */
const NAMES_IN_SUMMARY = 3;

/** `names` joined, the first few and then how many more. */
function shortList(names: string[]): string {
  const shown = names.slice(0, NAMES_IN_SUMMARY).join(", ");
  const rest = names.length - NAMES_IN_SUMMARY;
  return rest > 0 ? `${shown}, and ${rest} more` : shown;
}

/** The files a script uploads that the Test files folder does not hold:
 * a run fails at that upload, so it is said as a warning, with the names. */
function MissingFiles({ names, full = false }: { names: string[]; full?: boolean }) {
  return (
    <span className="inline-flex items-start gap-1 text-warning">
      <TriangleAlert aria-hidden className="mt-0.5 size-3.5 shrink-0" />
      <span className="min-w-0 break-words">
        {plural(names.length, "file")} missing: {full ? names.join(", ") : shortList(names)}
      </span>
    </span>
  );
}

/** The collapsed panel's lines: one per item, from the reads above, in
 * the full rows' own words. `missing` is set on the Test files line when a
 * script uploads a file the folder lacks: the line then names them. */
function summaryLines(
  s: AutoRunSetup,
): { label: string; status: Status; value: string; missing?: string[] }[] {
  const pickProject = "Pick a project first";
  // No environment is an answer, not a gap: runs then go to the saved
  // recipe's address, which the Site address line says.
  const env: [Status, string] = s.envs.isPending
    ? ["quiet", "Loading…"]
    : s.envs.isError
      ? ["warning", "Could not be read"]
      : s.activeEnv
        ? ["ready", s.activeEnv.name]
        : ["quiet", "None"];
  const site: [Status, string] = !s.setupReady
    ? ["missing", pickProject]
    : s.knownSiteUrl === undefined
      ? s.envs.isPending || s.recipe.isPending
        ? ["quiet", "Loading…"]
        : ["warning", "Could not be read"]
      : s.knownSiteUrl === ""
        ? ["missing", "Not set up yet"]
        : ["ready", s.site.start_url || s.knownSiteUrl];
  const signIn: [Status, string] =
    s.signIn === "none"
      ? ["missing", pickProject]
      : s.signIn === "saved"
        ? ["ready", "Recipe saved"]
        : s.signIn === "builtin"
          ? ["ready", "Built-in"]
          : s.recipe.isError
            ? ["warning", "Could not be read"]
            : ["quiet", "Loading…"];
  const accounts: [Status, string] = s.accounts.isError
    ? ["warning", "Could not be read"]
    : s.accountCount == null
      ? ["quiet", "Loading…"]
      : s.accountCount === 0
        ? ["missing", "None yet"]
        : ["ready", `${plural(s.accountCount, "account")} on this machine`];
  const areas: [Status, string] = !s.setupReady
    ? ["quiet", pickProject]
    : s.nav.isError
      ? ["warning", "Could not be read"]
      : s.areaCount == null
        ? ["quiet", "Loading…"]
        : s.areaCount === 0
          ? ["warning", "None recorded yet"]
          : ["ready", `${plural(s.areaCount, "area")} recorded`];
  const discoveryStale = (s.discoveryMap.data?.areas ?? []).some(
    (a) => a.area !== "" && a.explored_at != null && a.stale,
  );
  const discovery: [Status, string] = !s.setupReady
    ? ["quiet", pickProject]
    : s.discoveryMap.isError
      ? ["warning", "Could not be read"]
      : s.discoveryLine == null
        ? ["quiet", "Loading…"]
        : s.discoveryLine === "Not explored yet"
          ? ["quiet", s.discoveryLine]
          : [discoveryStale ? "warning" : "ready", s.discoveryLine];
  const components: [Status, string] = !s.setupReady
    ? ["quiet", pickProject]
    : s.components.isError
      ? ["warning", "Could not be read"]
      : s.componentsLine == null
        ? ["quiet", "Loading…"]
        : [s.componentsLine === "None yet" ? "quiet" : "ready", s.componentsLine];
  const missingFiles = s.readiness.missingTestFiles.length;
  const files: [Status, string] = !s.setupReady
    ? ["quiet", pickProject]
    : s.testFiles.isError
      ? ["warning", "Could not be read"]
      : missingFiles > 0
        ? ["warning", `${plural(missingFiles, "file")} missing`]
        : s.testFileCount == null
          ? ["quiet", "Loading…"]
          : s.testFileCount === 0
            ? ["quiet", "None yet"]
            : ["ready", plural(s.testFileCount, "file")];
  const db: [Status, string] = !s.activeEnv
    ? ["quiet", "No environment"]
    : s.databases.isPending
      ? ["quiet", "Loading…"]
      : s.databases.isError
        ? ["warning", "Could not be read"]
        : !s.activeDb
          ? ["warning", "Not set up any more"]
          : s.activeDb.server
            ? ["ready", s.activeDb.label]
            : ["warning", "Not set up yet"];
  return [
    { label: "Environment", status: env[0], value: env[1] },
    { label: "Site address", status: site[0], value: site[1] },
    { label: "Sign-in", status: signIn[0], value: signIn[1] },
    { label: "Accounts", status: accounts[0], value: accounts[1] },
    { label: "Areas", status: areas[0], value: areas[1] },
    { label: "Discovery", status: discovery[0], value: discovery[1] },
    { label: "Components", status: components[0], value: components[1] },
    {
      label: "Test files",
      status: files[0],
      value: files[1],
      missing: s.setupReady && !s.testFiles.isError && missingFiles > 0 ? s.readiness.missingTestFiles : undefined,
    },
    { label: "Database", status: db[0], value: db[1] },
  ];
}

export default function SetupPanel({
  setup: s,
  org,
  project,
  caseModules,
  open,
  onToggle,
  search,
  onSearch,
}: {
  setup: AutoRunSetup;
  org: string;
  project: string;
  /** The Module values of the listed cases, for the Areas dialog's picker. */
  caseModules: string[];
  open: boolean;
  onToggle: () => void;
  /** What the case list's search box holds, and its change. */
  search: string;
  onSearch: (text: string) => void;
}) {
  const headingId = useId();
  const bodyId = useId();
  const [accountsOpen, setAccountsOpen] = useState(false);
  const [recipeOpen, setRecipeOpen] = useState(false);
  const [recordOpen, setRecordOpen] = useState(false);
  const [navOpen, setNavOpen] = useState(false);
  const [discoveryOpen, setDiscoveryOpen] = useState(false);
  const [componentsOpen, setComponentsOpen] = useState(false);
  const [siteOpen, setSiteOpen] = useState(false);
  const [testFilesOpen, setTestFilesOpen] = useState(false);
  const [saveWordsOpen, setSaveWordsOpen] = useState(false);
  const [protectedOpen, setProtectedOpen] = useState(false);
  // While the assistant's discovery holds the Auto Run browser, End
  // discovery is the person's way out of one the assistant never ended.
  // Nothing is lost by ending it - the map keeps what was seen - so it asks
  // no confirm.
  const discovering = useDiscoveryActive();
  const [ending, setEnding] = useState(false);
  const [endProblem, setEndProblem] = useState<string | null>(null);
  const endDiscovery = async () => {
    setEndProblem(null);
    setEnding(true);
    try {
      await unwrapStr(commands.autoRunEndDiscovery());
    } catch (e) {
      setEndProblem(e instanceof Error ? e.message : String(e));
    }
    setEnding(false);
  };
  // Release Auto Run browser: the way out of a browser the app still holds
  // after it closed (refused with "close the supervised browser first" while
  // none is open). It clears every Auto Run browser record and closes only
  // the browsers the app itself opened, so, like Close browser and End
  // discovery, it asks no confirm. Always offered: a held browser that has
  // gone is exactly the one nothing on screen shows.
  const [releasing, setReleasing] = useState(false);
  const [released, setReleased] = useState<string | null>(null);
  const releaseBrowser = async () => {
    setEndProblem(null);
    setReleased(null);
    setReleasing(true);
    try {
      const said = await unwrapStr(commands.autoRunReleaseBrowser());
      setReleased(said ? `${said.charAt(0).toUpperCase()}${said.slice(1)}.` : null);
    } catch (e) {
      setEndProblem(e instanceof Error ? e.message : String(e));
    }
    setReleasing(false);
  };

  const { setupReady, recipe, envs, accounts, nav, testFiles, site, activeEnv } = s;
  const needsProject = setupReady ? undefined : "Pick an organization and project first";
  const extraSites = site.allowed_origins.length;
  const dbLine = s.databases.isPending
    ? "loading…"
    : s.databases.isError
      ? "could not be read"
      : !s.activeDb
        ? "not set up any more"
        : s.activeDb.server
          ? `${s.activeDb.label}: ${s.activeDb.database} on ${s.activeDb.server}`
          : `${s.activeDb.label}: not set up yet`;

  // The full rows carry the same status as the summary lines. Save words
  // and Protected records have no summary line: quiet, so every row's label
  // starts at the same place.
  const lines = summaryLines(s);
  const statusOf = (label: string): Status => lines.find((l) => l.label === label)?.status ?? "quiet";
  const dbStatus = statusOf("Database");

  return (
    <section
      aria-labelledby={headingId}
      className="space-y-3 rounded-md border border-border bg-surface p-4"
    >
      <div className="flex items-center gap-2">
        <h2 id={headingId} className="text-sm font-semibold text-text">
          Setup
        </h2>
        {/* There is no site address, no way to sign in, or no account, or
            one of them could not be read - said in words too. */}
        {s.attention && (
          <span className="inline-flex items-center gap-1 text-xs text-warning">
            <TriangleAlert aria-hidden className="size-3.5" />
            Needs attention
          </span>
        )}
        <Button
          size="sm"
          variant="ghost"
          className="ml-auto"
          aria-expanded={open}
          aria-controls={bodyId}
          onClick={onToggle}
        >
          {open ? <IconHideDetails aria-hidden /> : <IconShowDetails aria-hidden />}
          {/* The visible words are the whole name, so a voice command
              that reads them off the screen reaches the button. */}
          {open ? "Hide setup details" : "Show setup details"}
        </Button>
      </div>

      {/* Finding a case: open or shut, on either tab. */}
      <CaseSearch value={search} onChange={onSearch} wide />

      <div id={bodyId}>
        {!open ? (
          <ul aria-label="Setup summary" className="space-y-1.5">
            {lines.map(({ label, status, value, missing }) => (
              <li key={label} className="flex items-start gap-2 text-sm">
                {/* One line tall and centred in it, so the dot sits beside
                    the label's first line even when the value wraps. */}
                <StatusDot status={status} boxed />
                <span className="w-24 shrink-0 text-muted">{label}</span>
                {/* Never cut short: the site address in particular is only
                    useful whole. It breaks inside the URL; the rest wrap at
                    words. */}
                <span
                  className="min-w-0 flex-1 break-words text-text"
                >
                  {missing ? (
                    <MissingFiles names={missing} />
                  ) : label === "Site address" ? (
                    breakableUrl(value)
                  ) : (
                    value
                  )}
                </span>
                {SPOKEN[status] && <span className="sr-only">{SPOKEN[status]}</span>}
              </li>
            ))}
          </ul>
        ) : (
          <div className="space-y-3">
            <div className="space-y-3">
              {/* Read-only here: the environment and its database are
                  chosen on AI Bridge, which owns them - so no button. */}
              <SetupRow
                label="Environment"
                status={statusOf("Environment")}
                state={
                  envs.isError ? (
                    <span className="text-danger">The environments could not be read</span>
                  ) : envs.isPending ? (
                    <span className="text-muted">Loading…</span>
                  ) : activeEnv ? (
                    <>
                      <span className="font-medium">{activeEnv.name}</span>
                      <span className="block text-xs text-muted">
                        <StatusDot status={dbStatus} /> Database <span className="text-text">{dbLine}</span>
                        {SPOKEN[dbStatus] && <span className="sr-only">{SPOKEN[dbStatus]}</span>}. Both change on the AI Bridge
                        tab.
                      </span>
                    </>
                  ) : (
                    <span className="text-muted">None. Environments are chosen on the AI Bridge tab.</span>
                  )
                }
              />

              <SetupRow
                label="Site address"
                status={statusOf("Site address")}
                state={
                  !setupReady ? (
                    <span className="text-muted">{needsProject}</span>
                  ) : recipe.isLoading ? (
                    <span className="text-muted">Loading…</span>
                  ) : recipe.isError ? (
                    <span className="text-danger">The saved recipe could not be read</span>
                  ) : envs.isError && !site.start_url ? (
                    <span className="text-danger">The environments could not be read</span>
                  ) : site.start_url ? (
                    <>
                      <span className="id-mono break-all">{site.start_url}</span>
                      {extraSites > 0 && (
                        <span className="ml-2 text-xs text-faint">+{plural(extraSites, "allowed site")}</span>
                      )}
                    </>
                  ) : (
                    <span className="text-muted">Not set up yet</span>
                  )
                }
              >
                {/* The address is the active environment's, not the
                    recipe's, so it is set here with or without a saved
                    recipe: the built-in sign-in needs nothing more. */}
                <Button
                  size="sm"
                  variant="outline"
                  aria-label="Edit site address"
                  disabled={!setupReady}
                  title={needsProject}
                  onClick={() => setSiteOpen(true)}
                >
                  <IconSiteAddress aria-hidden />
                  Edit
                </Button>
              </SetupRow>

              <SetupRow
                label="Sign-in"
                status={statusOf("Sign-in")}
                state={
                  !setupReady ? (
                    <span className="text-muted">{needsProject}</span>
                  ) : recipe.isLoading ? (
                    <span className="text-muted">Loading…</span>
                  ) : recipe.isError ? (
                    <span className="text-danger">Could not be read - open it to see why</span>
                  ) : recipe.data ? (
                    "Recipe saved"
                  ) : (
                    // No saved recipe: the app's own runs. Recording or
                    // editing one saves this project's, which replaces it.
                    "Built-in"
                  )
                }
              >
                {/* Record: sign in by hand once and the recipe is written.
                    Edit: the recipe as JSON, for what a recording cannot say. */}
                <Button
                  size="sm"
                  variant="outline"
                  aria-label="Record sign-in"
                  disabled={!setupReady}
                  title={needsProject}
                  onClick={() => setRecordOpen(true)}
                >
                  <IconRecord aria-hidden />
                  Record
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  aria-label="Edit sign-in recipe"
                  disabled={!setupReady}
                  title={needsProject}
                  onClick={() => setRecipeOpen(true)}
                >
                  <IconRecipe aria-hidden />
                  Edit
                </Button>
              </SetupRow>

              <SetupRow
                label="Accounts"
                status={statusOf("Accounts")}
                state={
                  accounts.isError ? (
                    <span className="text-danger">The accounts could not be read</span>
                  ) : accounts.isPending || s.accountCount == null ? (
                    <span className="text-muted">Loading…</span>
                  ) : s.accountCount === 0 ? (
                    <span className="text-muted">None yet</span>
                  ) : (
                    `${plural(s.accountCount, "account")} on this machine`
                  )
                }
              >
                <Button size="sm" variant="outline" aria-label="Edit accounts" onClick={() => setAccountsOpen(true)}>
                  <IconAccounts aria-hidden />
                  Edit
                </Button>
              </SetupRow>

              <SetupRow
                label="Areas"
                status={statusOf("Areas")}
                state={
                  !setupReady ? (
                    <span className="text-muted">{needsProject}</span>
                  ) : nav.isError ? (
                    <span className="text-danger">The areas could not be read</span>
                  ) : nav.isPending || s.areaCount == null ? (
                    <span className="text-muted">Loading…</span>
                  ) : s.areaCount === 0 ? (
                    <span className="text-muted">None recorded yet</span>
                  ) : (
                    `${plural(s.areaCount, "area")} recorded`
                  )
                }
              >
                <Button
                  size="sm"
                  variant="outline"
                  aria-label="Edit areas"
                  disabled={!setupReady}
                  title={needsProject}
                  onClick={() => setNavOpen(true)}
                >
                  <IconModulePaths aria-hidden />
                  Edit
                </Button>
              </SetupRow>

              {/* What the assistant's discovery has seen of the live app,
                  area by area. */}
              <SetupRow
                label="Discovery"
                status={statusOf("Discovery")}
                state={
                  !setupReady ? (
                    <span className="text-muted">{needsProject}</span>
                  ) : s.discoveryMap.isError ? (
                    <span className="text-danger">The discovery map could not be read</span>
                  ) : s.discoveryLine == null ? (
                    <span className="text-muted">Loading…</span>
                  ) : s.discoveryLine === "Not explored yet" ? (
                    <span className="text-muted">{s.discoveryLine}</span>
                  ) : (
                    s.discoveryLine
                  )
                }
              >
                {discovering && (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={ending}
                    title="Close the assistant's discovery browser so Auto Run can run again"
                    onClick={() => void endDiscovery()}
                  >
                    <IconStop aria-hidden />
                    End discovery
                  </Button>
                )}
                <Button
                  size="sm"
                  variant="outline"
                  disabled={releasing}
                  title="Let go of an Auto Run browser the app still holds after it closed, and close the browsers the app opened"
                  onClick={() => void releaseBrowser()}
                >
                  <IconRelease aria-hidden />
                  Release Auto Run browser
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  aria-label="View discovery"
                  disabled={!setupReady}
                  title={needsProject}
                  onClick={() => setDiscoveryOpen(true)}
                >
                  <IconDiscoveryMap aria-hidden />
                  View
                </Button>
              </SetupRow>
              {endProblem && (
                <p role="alert" className="text-xs text-danger">
                  {endProblem}
                </p>
              )}
              {released && (
                <p role="status" className="text-xs text-muted">
                  {released}
                </p>
              )}

              {/* Steps saved once that scripts use by name. */}
              <SetupRow
                label="Components"
                status={statusOf("Components")}
                state={
                  !setupReady ? (
                    <span className="text-muted">{needsProject}</span>
                  ) : s.components.isError ? (
                    <span className="text-danger">The components could not be read</span>
                  ) : s.componentsLine == null ? (
                    <span className="text-muted">Loading…</span>
                  ) : s.componentsLine === "None yet" ? (
                    <span className="text-muted">{s.componentsLine}</span>
                  ) : (
                    s.componentsLine
                  )
                }
              >
                <Button
                  size="sm"
                  variant="outline"
                  aria-label="View components"
                  disabled={!setupReady}
                  title={needsProject}
                  onClick={() => setComponentsOpen(true)}
                >
                  <IconComponents aria-hidden />
                  View
                </Button>
              </SetupRow>

              <SetupRow
                label="Test files"
                status={statusOf("Test files")}
                state={
                  !setupReady ? (
                    <span className="text-muted">{needsProject}</span>
                  ) : testFiles.isError ? (
                    <span className="text-danger">The test files could not be read</span>
                  ) : testFiles.isPending || s.testFileCount == null ? (
                    <span className="text-muted">Loading…</span>
                  ) : (
                    <>
                      {s.testFileCount === 0 ? (
                        <span className="text-muted">None yet</span>
                      ) : (
                        plural(s.testFileCount, "file")
                      )}
                      {s.readiness.missingTestFiles.length > 0 && (
                        <span className="mt-1 block text-xs">
                          <MissingFiles names={s.readiness.missingTestFiles} full />
                        </span>
                      )}
                    </>
                  )
                }
              >
                <Button
                  size="sm"
                  variant="outline"
                  aria-label="Manage test files"
                  disabled={!setupReady}
                  title={needsProject}
                  onClick={() => setTestFilesOpen(true)}
                >
                  <IconTestFiles aria-hidden />
                  Manage
                </Button>
              </SetupRow>

              {/* What a script marked Must not save has stopped: the
                  built-in words, fixed, then the project's own. */}
              <SetupRow
                label="Save words"
                status={statusOf("Save words")}
                state={
                  !setupReady ? (
                    <span className="text-muted">{needsProject}</span>
                  ) : nav.isError ? (
                    <span className="text-danger">The save words could not be read</span>
                  ) : nav.isPending ? (
                    <span className="text-muted">Loading…</span>
                  ) : (
                    <>
                      <span className="text-muted">
                        {(nav.data?.built_in_save_words?.length
                          ? nav.data.built_in_save_words
                          : BUILT_IN_SAVE_WORDS
                        ).join(", ")}
                      </span>
                      {(nav.data?.save_words ?? []).length > 0 && (
                        <span>, {(nav.data?.save_words ?? []).join(", ")}</span>
                      )}
                    </>
                  )
                }
              >
                <Button
                  size="sm"
                  variant="outline"
                  aria-label="Edit save words"
                  disabled={!setupReady || nav.isPending || nav.isError}
                  title={needsProject}
                  onClick={() => setSaveWordsOpen(true)}
                >
                  <IconSaveWords aria-hidden />
                  Edit
                </Button>
              </SetupRow>

              {/* The records its tests must never edit or delete, handed to
                  the Playwright repo on export. */}
              <SetupRow
                label="Protected records"
                status={statusOf("Protected records")}
                state={
                  !setupReady ? (
                    <span className="text-muted">{needsProject}</span>
                  ) : nav.isError ? (
                    <span className="text-danger">The protected records could not be read</span>
                  ) : nav.isPending ? (
                    <span className="text-muted">Loading…</span>
                  ) : (nav.data?.protected ?? []).length === 0 ? (
                    <span className="text-muted">None</span>
                  ) : (
                    <span className="text-muted">{plural((nav.data?.protected ?? []).length, "record")}</span>
                  )
                }
              >
                <Button
                  size="sm"
                  variant="outline"
                  aria-label="Edit protected records"
                  disabled={!setupReady || nav.isPending || nav.isError}
                  title={needsProject}
                  onClick={() => setProtectedOpen(true)}
                >
                  <IconProtectedRecords aria-hidden />
                  Edit
                </Button>
              </SetupRow>
            </div>
            <p className="text-xs text-muted">
              An assistant&apos;s <span className="id-mono">/tcm:setup</span> command can walk you through this.
            </p>
          </div>
        )}
      </div>

      {accountsOpen && <AccountsDialog onClose={() => setAccountsOpen(false)} />}
      {recipeOpen && <RecipeEditor org={org} project={project} onClose={() => setRecipeOpen(false)} />}
      {recordOpen && <RecordSignInDialog org={org} project={project} onClose={() => setRecordOpen(false)} />}
      {testFilesOpen && <TestFilesDialog org={org} project={project} onClose={() => setTestFilesOpen(false)} />}
      {siteOpen && <SiteAddressDialog org={org} project={project} onClose={() => setSiteOpen(false)} />}
      {saveWordsOpen && (
        <SaveWordsDialog org={org} project={project} view={nav.data ?? null} onClose={() => setSaveWordsOpen(false)} />
      )}
      {protectedOpen && (
        <ProtectedRecordsDialog
          org={org}
          project={project}
          view={nav.data ?? null}
          onClose={() => setProtectedOpen(false)}
        />
      )}
      {discoveryOpen && (
        <DiscoveryDialog org={org} project={project} onClose={() => setDiscoveryOpen(false)} />
      )}
      {componentsOpen && (
        <ComponentsDialog org={org} project={project} onClose={() => setComponentsOpen(false)} />
      )}
      {navOpen && (
        <AreasDialog org={org} project={project} caseModules={caseModules} onClose={() => setNavOpen(false)} />
      )}
    </section>
  );
}

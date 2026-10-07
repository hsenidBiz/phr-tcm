// The setup a run needs, beside the Test cases list: the site address, the
// sign-in, the accounts, the areas, the test files and the save words, with
// the database the active environment points at.
//
// Collapsed, it is one line per item with a status dot. Expanded, it is the
// full rows with their buttons, each opening its own dialog. The screen
// decides once, when the setup has loaded, whether it starts expanded (see
// `index.tsx`); after that only the person's toggle moves it.
//
// `useAutoRunSetup` reads everything the panel and the readiness strip say,
// so the screen and the panel never read the same thing twice. Readiness
// itself is `useAutoRunReadiness`'s, called once, here.

import { useQuery } from "@tanstack/react-query";
import { TriangleAlert } from "lucide-react";
import { useId, useMemo, useState, type ReactNode, type Ref } from "react";
import { commands, type CaseScript } from "../../bindings";
import { Button } from "../../components/ui/button";
import { cn } from "../../lib/cn";
import { activeEnvironment, effectiveSite, useEnvironments } from "../../lib/environments";
import { unwrapStr } from "../../lib/ipc";
import {
  IconAccounts,
  IconHideDetails,
  IconModulePaths,
  IconRecipe,
  IconRecord,
  IconSaveWords,
  IconShowDetails,
  IconSiteAddress,
  IconTestFiles,
} from "../../lib/actionIcons";
import AccountsDialog from "./AccountsDialog";
import AreasDialog from "./AreasDialog";
import RecipeEditor from "./RecipeEditor";
import RecordSignInDialog from "./RecordSignInDialog";
import SaveWordsDialog, { BUILT_IN_SAVE_WORDS } from "./SaveWordsDialog";
import SiteAddressDialog from "./SiteAddressDialog";
import TestFilesDialog, { useTestFiles } from "./TestFilesDialog";
import { useAutoRunReadiness } from "./useAutoRunReadiness";

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** Everything the Setup panel and the readiness strip report. All of it
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
  /** Reads that failed, each as the sentence its row already says. The
   * first three are what a run cannot go without; they also flag the
   * panel. */
  const essentialUnreadable = [
    ...(envs.isError ? ["The environments could not be read"] : []),
    ...(setupReady && recipe.isError ? ["The saved recipe could not be read"] : []),
    ...(accounts.isError ? ["The accounts could not be read"] : []),
  ];
  const unreadable = [
    ...essentialUnreadable,
    ...(setupReady && testFiles.isError ? ["The test files could not be read"] : []),
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
    unreadable,
    /** Something a run cannot go without is missing, or could not be read. */
    attention: readiness.essentialMissing || essentialUnreadable.length > 0,
  };
}

export type AutoRunSetup = ReturnType<typeof useAutoRunSetup>;

/** One of the full Setup rows: what it is and its buttons on one line,
 * then where it stands, at the panel's full width below them. The panel is
 * a narrow column, so a value never shares its line with the buttons: an
 * address or a list of words gets the whole width to wrap in. A labelled
 * group, so a screen reader hears the row's name with its state and its
 * buttons. */
function SetupRow({ label, state, children }: { label: string; state: ReactNode; children: ReactNode }) {
  return (
    <div
      role="group"
      aria-label={label}
      className="space-y-1 border-t border-border/60 pt-3 first-of-type:border-t-0 first-of-type:pt-0"
    >
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1 text-xs font-medium text-muted">{label}</span>
        <span className="flex shrink-0 flex-wrap justify-end gap-2">{children}</span>
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

/** A web address as text with a break opportunity after each `/`, `.`, `?`,
 * `&` and `=`, so a long one wraps at its own punctuation instead of in the
 * middle of a word. */
export function breakableUrl(url: string): ReactNode[] {
  return url.split(/(?<=[/.?&=])/).flatMap((part, i) => (i === 0 ? [part] : [<wbr key={i} />, part]));
}

/** The collapsed panel's lines: one per item, from the reads above, in
 * the full rows' own words. Not the readiness strip's, which sits beside
 * it: two places saying "2 accounts" read as two different things. */
function summaryLines(s: AutoRunSetup): { label: string; status: Status; value: string }[] {
  const pickProject = "Pick a project first";
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
  const missingFiles = s.readiness.missingTestFiles.length;
  const files: [Status, string] = !s.setupReady
    ? ["quiet", pickProject]
    : s.testFiles.isError
      ? ["warning", "Could not be read"]
      : missingFiles > 0
        ? ["missing", `${plural(missingFiles, "file")} missing`]
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
    { label: "Site address", status: site[0], value: site[1] },
    { label: "Sign-in", status: signIn[0], value: signIn[1] },
    { label: "Accounts", status: accounts[0], value: accounts[1] },
    { label: "Areas", status: areas[0], value: areas[1] },
    { label: "Test files", status: files[0], value: files[1] },
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
  toggleRef,
  panelRef,
}: {
  setup: AutoRunSetup;
  org: string;
  project: string;
  /** The Module values of the listed cases, for the Areas dialog's picker. */
  caseModules: string[];
  open: boolean;
  onToggle: () => void;
  toggleRef?: Ref<HTMLButtonElement>;
  panelRef?: Ref<HTMLElement>;
}) {
  const headingId = useId();
  const bodyId = useId();
  const [accountsOpen, setAccountsOpen] = useState(false);
  const [recipeOpen, setRecipeOpen] = useState(false);
  const [recordOpen, setRecordOpen] = useState(false);
  const [navOpen, setNavOpen] = useState(false);
  const [siteOpen, setSiteOpen] = useState(false);
  const [testFilesOpen, setTestFilesOpen] = useState(false);
  const [saveWordsOpen, setSaveWordsOpen] = useState(false);

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

  return (
    <section
      ref={panelRef}
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
          ref={toggleRef}
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

      <div id={bodyId}>
        {!open ? (
          <ul aria-label="Setup summary" className="space-y-1.5">
            {summaryLines(s).map(({ label, status, value }) => (
              <li key={label} className="flex items-start gap-2 text-sm">
                {/* One line tall and centred in it, so the dot sits beside
                    the label's first line even when the value wraps. */}
                <span aria-hidden className="flex h-5 shrink-0 items-center">
                  <span className={cn("size-2 rounded-full", DOT[status])} />
                </span>
                <span className="w-24 shrink-0 text-muted">{label}</span>
                {/* Never cut short: the site address in particular is only
                    useful whole. It breaks inside the URL; the rest wrap at
                    words. */}
                <span
                  className="min-w-0 flex-1 break-words text-text"
                >
                  {label === "Site address" ? breakableUrl(value) : value}
                </span>
                {SPOKEN[status] && <span className="sr-only">{SPOKEN[status]}</span>}
              </li>
            ))}
          </ul>
        ) : (
          <div className="space-y-3">
            {/* Read-only here: the environment and its database are chosen
                on AI Bridge, which owns them. */}
            {activeEnv && (
              <p className="text-xs text-muted">
                Environment <span className="font-medium text-text">{activeEnv.name}</span>, database{" "}
                <span className="font-medium text-text">{dbLine}</span>. Both change on the AI Bridge tab.
              </p>
            )}
            <div className="space-y-3">
              <SetupRow
                label="Site address"
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

              <SetupRow
                label="Test files"
                state={
                  !setupReady ? (
                    <span className="text-muted">{needsProject}</span>
                  ) : testFiles.isError ? (
                    <span className="text-danger">The test files could not be read</span>
                  ) : testFiles.isPending || s.testFileCount == null ? (
                    <span className="text-muted">Loading…</span>
                  ) : s.testFileCount === 0 ? (
                    <span className="text-muted">None yet</span>
                  ) : (
                    plural(s.testFileCount, "file")
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
      {navOpen && (
        <AreasDialog org={org} project={project} caseModules={caseModules} onClose={() => setNavOpen(false)} />
      )}
    </section>
  );
}

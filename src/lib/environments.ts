// Environments in the webview: the query that lists them, the switch, and
// the two rules that tie an environment to the Database Read Access card.
//
// An environment names its database by id, and the card's choice is the
// webview's copy of the ACTIVE environment's database. Switching an
// environment moves the card; changing the card changes the active
// environment (AiBridge) - one setting, not two. Rust owns the list and
// every password: this file only ever sees `EnvView`s, which carry a
// "default password is set" flag and nothing more.

import { useQuery, type QueryClient } from "@tanstack/react-query";
import { commands, type EnvInput, type EnvListView, type EnvView } from "../bindings";
import { saveSelectedDb, selectedDbSnapshot } from "./dbServer";
import { logUi } from "./uiLog";

/** React Query keys for the environments. */
export const envKeys = {
  list: ["environments"] as const,
  /** The active environment's accounts (passwords included - the
   * Accounts dialog edits them). */
  accounts: ["autorun-accounts"] as const,
  /** The assistant's proposed accounts for the active environment. */
  proposals: ["env-proposals"] as const,
  /** Every API template overview: each names the environment's site and
   * account keys. */
  apiTemplates: ["api-templates"] as const,
};

/** Forgets everything read for the environment that was active: after a
 * switch, a screen must never show - or save back - the previous
 * environment's accounts. A screen showing one of these reads it afresh. */
export async function forgetEnvironmentData(qc: QueryClient): Promise<void> {
  await Promise.all(
    [envKeys.accounts, envKeys.proposals, envKeys.apiTemplates].map((queryKey) =>
      qc.resetQueries({ queryKey }),
    ),
  );
}

/** Set once the first list has been compared with the Database Read Access
 * card, so that comparison can never override a later deliberate change. */
const RECONCILED_KEY = "tcm-v2-env-db-reconciled";

function reconciled(): boolean {
  try {
    return localStorage.getItem(RECONCILED_KEY) === "1";
  } catch {
    return false;
  }
}

function markReconciled(): void {
  try {
    localStorage.setItem(RECONCILED_KEY, "1");
  } catch {
    // storage unavailable -> the check simply runs again next launch
  }
}

/** The ids of the databases this build knows, or null when the list could
 * not be read. */
async function knownDatabaseIds(): Promise<string[] | null> {
  try {
    return ((await commands.dbDatabases()) ?? []).map((d) => d.id);
  } catch {
    return null;
  }
}

/** An environment as the form of `envSave` takes it back. */
export function toInput(env: EnvView): EnvInput {
  return {
    id: env.id,
    name: env.name,
    start_url: env.start_url,
    allowed_origins: env.allowed_origins,
    db_id: env.db_id,
    test_environment: env.test_environment,
  };
}

const EMPTY: EnvListView = { active: "", environments: [] };

/** The environments, creating Default on first use.
 *
 * The Database Read Access card's choice is passed only when it is a database
 * this build knows: Rust takes whatever id it is given for a new Default,
 * and an id that names nothing must never be written into an environment
 * (Rust falls back to the first shipped database on null).
 *
 * On the first list ever, a Default made earlier WITHOUT the card's choice
 * (Auto Run can reach the environments before this tab does) is brought in
 * line with the card, once. */
export async function loadEnvironments(): Promise<EnvListView> {
  const known = await knownDatabaseIds();
  const card = selectedDbSnapshot();
  const cardChoice = card && known?.includes(card) ? card : null;
  const res = await commands.envList(cardChoice);
  if (res.status === "error") throw new Error(res.error);
  let view = res.data ?? EMPTY;

  if (!reconciled() && view.environments.length > 0) {
    const active = view.environments.find((e) => e.id === view.active);
    if (cardChoice && active && active.name.toLowerCase() === "default" && active.db_id !== cardChoice) {
      try {
        const saved = await commands.envSave({ ...toInput(active), db_id: cardChoice });
        if (saved.status === "ok") {
          view = saved.data;
          markReconciled();
          logUi("environments: Default now uses the database chosen on the Database Read Access card");
        } else {
          logUi(`environments: could not bring Default in line with the database card: ${saved.error}`);
        }
      } catch {
        logUi("environments: could not bring Default in line with the database card");
      }
    } else if (known !== null) {
      // Only a comparison that could actually be made counts: with the
      // database list unreadable the card's choice was not checked, and
      // the next launch has to look again.
      markReconciled();
    }
  }
  return view;
}

/** The environments and which is active, shared by everything that shows
 * them. */
export function useEnvironments() {
  return useQuery({ queryKey: envKeys.list, queryFn: loadEnvironments, retry: false });
}

/** The name the title bar shows: the active environment's, but only once
 * there is more than one to tell apart. */
export function activeEnvironmentLabel(view: EnvListView | undefined): string | null {
  if (!view || view.environments.length < 2) return null;
  return view.environments.find((e) => e.id === view.active)?.name ?? null;
}

/** The title-bar label for the running app. */
export function useActiveEnvironmentName(): string | null {
  return activeEnvironmentLabel(useEnvironments().data);
}

/** Makes `id` the active environment and moves the Database Read Access card
 * to its database. An environment whose database is no longer in the list
 * (a saved custom login removed or reset) still switches; it sets no
 * database, and `dbMissing` says so. `view` is the list as Rust now has it.
 *
 * Throws Rust's refusal (a run in progress, say) as an Error. */
export async function switchEnvironment(id: string): Promise<{ dbMissing: boolean; view: EnvListView }> {
  const res = await commands.envSetActive(id);
  if (res.status === "error") throw new Error(res.error);
  const view = res.data ?? EMPTY;
  const env = view.environments.find((e) => e.id === view.active);
  if (!env) return { dbMissing: false, view };
  const known = await knownDatabaseIds();
  // Unsure (the list could not be read) keeps the environment's choice:
  // naming a database that turns out to be gone is recoverable.
  if (known === null || known.includes(env.db_id)) {
    saveSelectedDb(env.db_id);
    return { dbMissing: false, view };
  }
  saveSelectedDb("");
  return { dbMissing: true, view };
}

/** Whether the active environment names a database that is not set up any
 * more. False while the databases are still loading. */
export function activeDbMissing(view: EnvListView | undefined, databaseIds: string[] | undefined): boolean {
  if (!view || !databaseIds) return false;
  const env = view.environments.find((e) => e.id === view.active);
  return Boolean(env) && !databaseIds.includes(env!.db_id);
}

/** The active environment, once the list is known. */
export function activeEnvironment(view: EnvListView | undefined): EnvView | undefined {
  return view?.environments.find((e) => e.id === view.active);
}

/** The site a run goes to: the active environment's address when it has
 * one, else the sign-in recipe's. Same rule as Rust's `effective_recipe`:
 * an environment with an address brings its own allowed sites too, and the
 * recipe's belong to another site. Empty when neither names a site. */
export function effectiveSite(
  view: EnvListView | undefined,
  recipe: { start_url: string; allowed_origins?: string[] } | null | undefined,
): { start_url: string; allowed_origins: string[] } {
  const env = activeEnvironment(view);
  const own = env?.start_url.trim() ?? "";
  if (env && own) return { start_url: own, allowed_origins: env.allowed_origins };
  return { start_url: recipe?.start_url ?? "", allowed_origins: recipe?.allowed_origins ?? [] };
}

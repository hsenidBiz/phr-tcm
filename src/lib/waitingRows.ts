// What "Waiting for you" draws, the sidebar counts and the bell announces.
// Every proposed account is its own item from Rust, but a person decides
// them together in one window, so they collapse into a single row: an
// assistant proposing forty accounts is one thing waiting, not forty.

import type { WaitingItem } from "../bindings";

export type WaitingRow =
  | { kind: "accounts"; count: number; proposed_ms: number | null }
  | { kind: "item"; item: WaitingItem };

/** The sentence for the grouped accounts row and its bell notice. */
export function accountsWaitingTitle(count: number): string {
  return count === 1
    ? "1 proposed account is waiting for you to accept"
    : `${count} proposed accounts are waiting for you to accept`;
}

const stamp = (ms: number | null | undefined) => ms ?? 0;

/** Display rows, newest first. The accounts row sits where its newest
 * proposal falls; sign-ins and divergences stay one row each. Ties keep the
 * order the items arrived in. */
export function waitingRows(items: readonly WaitingItem[]): WaitingRow[] {
  const rows: WaitingRow[] = [];
  let accounts: Extract<WaitingRow, { kind: "accounts" }> | null = null;
  for (const item of items) {
    if (item.target.kind === "accounts") {
      if (!accounts) {
        accounts = { kind: "accounts", count: 0, proposed_ms: null };
        rows.push(accounts);
      }
      accounts.count += 1;
      if (stamp(item.proposed_ms) > stamp(accounts.proposed_ms)) accounts.proposed_ms = item.proposed_ms;
    } else {
      rows.push({ kind: "item", item });
    }
  }
  const time = (r: WaitingRow) => stamp(r.kind === "accounts" ? r.proposed_ms : r.item.proposed_ms);
  return rows.map((r, i) => ({ r, i })).sort((a, b) => time(b.r) - time(a.r) || a.i - b.i).map((x) => x.r);
}

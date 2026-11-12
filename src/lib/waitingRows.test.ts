import { expect, test } from "vitest";
import type { WaitingItem } from "../bindings";
import { accountsWaitingTitle, waitingRows } from "./waitingRows";

const acct = (key: string, ms: number | null): WaitingItem => ({
  key,
  title: key,
  proposed_ms: ms,
  target: { kind: "accounts" },
});
const div: WaitingItem = {
  key: "d1",
  title: "d",
  proposed_ms: 500,
  target: { kind: "divergence", id: "d1", case_id: 1, step: 1, pbi: null },
};

test("forty proposed accounts and a divergence are two rows", () => {
  const items = [...Array.from({ length: 40 }, (_, i) => acct(`a${i}`, 100 + i)), div];
  expect(waitingRows(items)).toHaveLength(2);
});

test("the accounts row carries the count and newest time, and sorts by it", () => {
  const rows = waitingRows([acct("a", 100), div, acct("b", 900), acct("c", 50)]);
  expect(rows).toEqual([
    { kind: "accounts", count: 3, proposed_ms: 900 },
    { kind: "item", item: div },
  ]);
  expect(waitingRows([acct("a", 100), div])[0]).toEqual({ kind: "item", item: div });
});

test("the sentence is singular for one", () => {
  expect(accountsWaitingTitle(1)).toBe("1 proposed account is waiting for you to accept");
  expect(accountsWaitingTitle(2)).toBe("2 proposed accounts are waiting for you to accept");
});

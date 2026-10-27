import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

import type { AdoError, PlanWithSuites } from "../bindings";
import { CACHE, cacheEntry, cacheKeys, cacheWrite } from "./cache";

type Answer = { status: "ok"; data: PlanWithSuites[] } | { status: "error"; error: AdoError };

const h = vi.hoisted(() => ({
  list: vi.fn(),
}));

vi.mock("../bindings", () => ({
  commands: { listPlansWithSuites: h.list },
}));

import { fetchPlansAll, MAX_FANOUT_PROJECTS, projectOfPlan } from "./plansAll";

const plan = (id: number, name = `Plan ${id}`): PlanWithSuites => ({
  plan: { id, name, area_path: "x", root_suite_id: id + 1 },
  suites: [],
});
const ok = (data: PlanWithSuites[]): Answer => ({ status: "ok", data });
const fail = (error: AdoError): Answer => ({ status: "error", error });

/** Answers each project from a table; a project not in it has no plans. */
function answerFrom(table: Record<string, Answer>) {
  h.list.mockImplementation((_org: string, project: string) =>
    Promise.resolve(table[project] ?? ok([])),
  );
}

const writtenKeys = () =>
  Object.keys(localStorage)
    .filter((k) => k.startsWith("tcm-v2-cache:") && k !== "tcm-v2-cache-index")
    .map((k) => k.slice("tcm-v2-cache:".length))
    .sort();

beforeEach(() => {
  localStorage.clear();
  h.list.mockReset();
});
afterEach(() => {
  localStorage.clear();
});

describe("fetchPlansAll", () => {
  test("a fresh per-project cache is used without asking", async () => {
    cacheWrite(cacheKeys.plansSuites("acme", "Payroll"), [plan(1)]);
    cacheWrite(cacheKeys.plansSuites("acme", "Web"), [plan(2)]);
    const asked: string[] = [];
    const onProject = (p: string) => asked.push(p);

    const all = await fetchPlansAll("acme", ["Web", "Payroll"], onProject);

    expect(h.list).not.toHaveBeenCalled();
    expect(asked).toEqual([]);
    expect(all.groups.map((g) => g.project)).toEqual(["Payroll", "Web"]);
    expect(all.groups[0].plans[0].plan.id).toBe(1);
    expect(all).toMatchObject({ shown: 2, total: 2 });
  });

  test("a stale or missing project is fetched and written under its own key", async () => {
    // Payroll is stale (older than staleMs but inside ttlMs); Web has nothing.
    cacheWrite(cacheKeys.plansSuites("acme", "Payroll"), [plan(1)]);
    const stored = JSON.parse(localStorage.getItem("tcm-v2-cache:plans-suites:acme/Payroll")!);
    stored.at = Date.now() - CACHE.structure.staleMs - 60_000;
    localStorage.setItem("tcm-v2-cache:plans-suites:acme/Payroll", JSON.stringify(stored));
    answerFrom({ Payroll: ok([plan(5)]), Web: ok([plan(6)]) });
    const asked: Array<[string, number, number]> = [];

    const all = await fetchPlansAll("acme", ["Web", "Payroll"], (p, i, n) => asked.push([p, i, n]));

    expect(h.list.mock.calls).toEqual([
      ["acme", "Payroll"],
      ["acme", "Web"],
    ]);
    expect(writtenKeys()).toEqual(["plans-suites:acme/Payroll", "plans-suites:acme/Web"]);
    expect(
      cacheEntry<PlanWithSuites[]>("plans-suites:acme/Payroll", CACHE.structure.ttlMs)?.data[0].plan.id,
    ).toBe(5);
    // Progress is reported for each project actually read, counted among
    // the projects the list covers.
    expect(asked).toEqual([
      ["Payroll", 1, 2],
      ["Web", 2, 2],
    ]);
    expect(all.groups.map((g) => g.plans[0].plan.id)).toEqual([5, 6]);
  });

  test("projects are scanned one at a time", async () => {
    const pending: Array<(a: Answer) => void> = [];
    let inFlight = 0;
    let peak = 0;
    h.list.mockImplementation(() => {
      inFlight++;
      peak = Math.max(peak, inFlight);
      return new Promise<Answer>((resolve) => {
        pending.push((a) => {
          inFlight--;
          resolve(a);
        });
      });
    });

    const done = fetchPlansAll("acme", ["A", "B", "C"]);
    for (let i = 0; i < 3; i++) {
      // Let the call start; only one request may be waiting at a time.
      await vi.waitFor(() => expect(pending.length).toBe(1));
      expect(inFlight).toBe(1);
      pending.shift()!(ok([plan(i + 1)]));
    }
    const all = await done;

    expect(peak).toBe(1);
    expect(h.list).toHaveBeenCalledTimes(3);
    expect(all.groups.map((g) => g.project)).toEqual(["A", "B", "C"]);
  });

  test("the project cap is honoured and reported", async () => {
    const names = Array.from({ length: MAX_FANOUT_PROJECTS + 5 }, (_, i) =>
      `P${String(i).padStart(3, "0")}`,
    );
    // Hand them over shuffled: the cut must be by name, not by arrival.
    const shuffled = [...names].reverse();
    answerFrom({});

    const all = await fetchPlansAll("acme", shuffled);

    expect(h.list).toHaveBeenCalledTimes(MAX_FANOUT_PROJECTS);
    const asked = h.list.mock.calls.map((c) => c[1]);
    expect(asked).toEqual(names.slice(0, MAX_FANOUT_PROJECTS));
    expect(all).toMatchObject({ shown: MAX_FANOUT_PROJECTS, total: MAX_FANOUT_PROJECTS + 5 });
    expect(all.groups).toEqual([]);
  });

  test("a forbidden project is skipped and counted out", async () => {
    answerFrom({
      A: ok([plan(1)]),
      B: fail({ kind: "Forbidden" }),
      C: fail({ kind: "NotFound" }),
      D: ok([]),
    });

    const all = await fetchPlansAll("acme", ["A", "B", "C", "D"]);

    // D answered with no plans: it counts as shown but has no group.
    expect(all.groups.map((g) => g.project)).toEqual(["A"]);
    expect(all).toMatchObject({ shown: 2, total: 4 });
    // A skipped project leaves nothing behind in the cache.
    expect(writtenKeys()).toEqual(["plans-suites:acme/A", "plans-suites:acme/D"]);
  });

  test("an expired sign-in fails the whole list", async () => {
    answerFrom({ A: ok([plan(1)]), B: fail({ kind: "Unauthorized" }), C: ok([plan(3)]) });

    await expect(fetchPlansAll("acme", ["A", "B", "C"])).rejects.toThrow(
      "Your session has expired - sign in again.",
    );

    // Nothing after the failure was asked.
    expect(h.list.mock.calls.map((c) => c[1])).toEqual(["A", "B"]);
  });

  test("any other failure fails the whole list too", async () => {
    answerFrom({ A: fail({ kind: "RateLimited", detail: { retry_after_secs: 7 } }) });

    await expect(fetchPlansAll("acme", ["A", "B"])).rejects.toThrow("Rate limited - retry in 7s.");
  });
});

describe("projectOfPlan", () => {
  test("a plan's project is found by its id", async () => {
    answerFrom({ A: ok([plan(10), plan(11)]), B: ok([plan(20)]) });
    const all = await fetchPlansAll("acme", ["B", "A"]);

    expect(projectOfPlan(all, 11)).toBe("A");
    expect(projectOfPlan(all, 20)).toBe("B");
    expect(projectOfPlan(all, 99)).toBeNull();
    expect(projectOfPlan(undefined, 11)).toBeNull();
  });
});

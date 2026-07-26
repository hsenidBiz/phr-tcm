import { afterEach, expect, test, vi } from "vitest";
import { cacheRead, cacheWrite } from "./localCache";
import { CACHE, persistentQuery } from "./persistentQuery";

afterEach(() => localStorage.clear());

test("a fetch writes the result to disk for the next launch", async () => {
  const opts = persistentQuery({
    key: "k",
    fetcher: async () => ["a", "b"],
    ...CACHE.structure,
  });
  expect(opts.initialData()).toBeUndefined(); // cold: nothing seeded
  await opts.queryFn();
  expect(cacheRead<string[]>("k", CACHE.structure.ttlMs)).toEqual(["a", "b"]);
});

test("a stored entry seeds initialData WITH its real age", () => {
  cacheWrite("k", ["seeded"]);
  const opts = persistentQuery({
    key: "k",
    fetcher: async () => ["fresh"],
    ...CACHE.structure,
  });
  expect(opts.initialData()).toEqual(["seeded"]);
  // The age is the write time, not "now" - otherwise React Query would
  // treat a week-old seed as freshly fetched and never revalidate.
  const at = opts.initialDataUpdatedAt();
  expect(at).toBeTypeOf("number");
  expect(Math.abs(Date.now() - at!)).toBeLessThan(5_000);
});

test("an entry older than the TTL is not served at all", () => {
  cacheWrite("k", ["ancient"]);
  const raw = JSON.parse(localStorage.getItem("tcm-v2-cache:k")!);
  raw.at = Date.now() - (CACHE.structure.ttlMs + 60_000);
  localStorage.setItem("tcm-v2-cache:k", JSON.stringify(raw));

  const opts = persistentQuery({ key: "k", fetcher: async () => [], ...CACHE.structure });
  expect(opts.initialData()).toBeUndefined();
});

test("structure is cached for hours; outcomes revalidate immediately", () => {
  // The distinction the feature rests on: the suite tree should not
  // refetch on every visit, run results should.
  expect(CACHE.structure.staleMs).toBeGreaterThan(60 * 60_000);
  expect(CACHE.outcomes.staleMs).toBe(0);
});

test("demo mode neither seeds nor stores", async () => {
  localStorage.setItem("tcm-v2-dev-demo", "on");
  const fetcher = vi.fn(async () => ["real"]);
  const opts = persistentQuery({ key: "k", fetcher, ...CACHE.structure });
  await opts.queryFn();
  expect(opts.initialData()).toBeUndefined();
  expect(localStorage.getItem("tcm-v2-cache:k")).toBeNull();
});

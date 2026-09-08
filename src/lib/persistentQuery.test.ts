import { afterEach, expect, test, vi } from "vitest";
import { cacheRead, cacheWrite, claimCacheFor } from "./localCache";
import { CACHE, persistentQuery } from "./persistentQuery";

afterEach(() => localStorage.clear());

/** Cache keys carry org and project, which is not the same as carrying the
 * PERSON. Two accounts on one Windows profile used to read each other's
 * plans, suites and outcomes - fetched with a token the second one never
 * held, and painted instantly from the seed before a request could have
 * been refused. */
test("signing in as someone else drops the previous account's cache", () => {
  claimCacheFor("first@example.com");
  cacheWrite("plans-suites:acme/Payments", ["Plan A"]);
  expect(cacheRead("plans-suites:acme/Payments", CACHE.structure.ttlMs)).toEqual(["Plan A"]);

  // Same person again, however many times: their cache survives.
  claimCacheFor("first@example.com");
  claimCacheFor("first@example.com");
  expect(cacheRead("plans-suites:acme/Payments", CACHE.structure.ttlMs)).toEqual(["Plan A"]);

  // Someone else: gone, even though org and project are identical.
  claimCacheFor("second@example.com");
  expect(cacheRead("plans-suites:acme/Payments", CACHE.structure.ttlMs)).toBeNull();

  // Signed out (no account yet) must not wipe what the signed-in user has.
  cacheWrite("plans-suites:acme/Payments", ["Plan B"]);
  claimCacheFor(null);
  expect(cacheRead("plans-suites:acme/Payments", CACHE.structure.ttlMs)).toEqual(["Plan B"]);

  // The address itself is never written to disk.
  expect(JSON.stringify(localStorage)).not.toContain("example.com");
});

/** Only this module's own entries are its to drop. */
test("claiming the cache leaves everything else in storage alone", () => {
  localStorage.setItem("tcm-v2-draft", "the user's queue");
  localStorage.setItem("tcm-v2-theme", "dark");
  claimCacheFor("first@example.com");
  cacheWrite("points:acme/Payments/1/2", [1, 2, 3]);
  claimCacheFor("second@example.com");
  expect(localStorage.getItem("tcm-v2-draft")).toBe("the user's queue");
  expect(localStorage.getItem("tcm-v2-theme")).toBe("dark");
});

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

import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, test, vi } from "vitest";
import {
  CACHE,
  cacheKeys,
  cacheRead,
  cacheRemove,
  cacheWrite,
  claimCacheFor,
  persistentQuery,
  suspendCache,
} from "./cache";

afterEach(() => {
  suspendCache(false);
  localStorage.clear();
});

test("round-trips within the TTL and expires after it", () => {
  cacheWrite("k", { a: 1 });
  expect(cacheRead<{ a: number }>("k", 60_000)).toEqual({ a: 1 });
  // An entry older than the TTL reads as a miss.
  const raw = JSON.parse(localStorage.getItem("tcm-v2-cache:k")!);
  raw.at = Date.now() - 120_000;
  localStorage.setItem("tcm-v2-cache:k", JSON.stringify(raw));
  expect(cacheRead("k", 60_000)).toBeNull();
});

test("demo mode never reads or writes the cache", () => {
  cacheWrite("real", "data");
  localStorage.setItem("tcm-v2-dev-demo", "on");
  expect(cacheRead("real", 60_000)).toBeNull(); // real data hidden from demo
  cacheWrite("demo-key", "demo-data");
  localStorage.setItem("tcm-v2-dev-demo", "off");
  expect(cacheRead("demo-key", 60_000)).toBeNull(); // demo data never stored
  expect(cacheRead("real", 60_000)).toBe("data"); // real data intact
});

test("unreadable entries are dropped, not served", () => {
  localStorage.setItem("tcm-v2-cache:bad", "{not json");
  expect(cacheRead("bad", 60_000)).toBeNull();
});

test("a suspended cache neither reads nor writes", () => {
  cacheWrite("tour-check", { a: 1 });
  expect(cacheRead("tour-check", 60_000)).toEqual({ a: 1 });

  suspendCache(true);
  expect(cacheRead("tour-check", 60_000)).toBeNull();
  cacheWrite("tour-check", { a: 2 });

  suspendCache(false);
  // The write while suspended was dropped - the earlier value survives.
  expect(cacheRead("tour-check", 60_000)).toEqual({ a: 1 });
});

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

/** Work-item details carry inline images as data: URIs; one multi-megabyte
 * write would trip the quota handler, which clears the whole cache. */
test("a store predicate keeps oversized results off the disk but still returns them", async () => {
  const big = "x".repeat(50);
  const opts = persistentQuery({
    key: "k",
    fetcher: async () => big,
    ...CACHE.outcomes,
    store: (d) => d.length <= 10,
  });
  expect(await opts.queryFn()).toBe(big);
  expect(localStorage.getItem("tcm-v2-cache:k")).toBeNull();

  const small = persistentQuery({
    key: "k2",
    fetcher: async () => "tiny",
    ...CACHE.outcomes,
    store: (d) => d.length <= 10,
  });
  await small.queryFn();
  expect(cacheRead("k2", CACHE.outcomes.ttlMs)).toBe("tiny");
});

test("shelf lives say what the data does", () => {
  // Structure should not refetch on every visit; run results should.
  expect(CACHE.structure.staleMs).toBeGreaterThan(60 * 60_000);
  expect(CACHE.outcomes.staleMs).toBe(0);
  // Org/project lists and members: a day from disk with no request at all.
  expect(CACHE.reference).toEqual({ ttlMs: 24 * 60 * 60_000, staleMs: 24 * 60 * 60_000 });
  // Pipelines of a finished PR never change: a month, never stale.
  expect(CACHE.finished).toEqual({ ttlMs: 30 * 24 * 60 * 60_000, staleMs: Infinity });
});

/** These strings are what earlier versions wrote. Changing one silently
 * throws away every user's cache for that data on upgrade. */
test("cache keys are the strings earlier versions stored", () => {
  expect(cacheKeys.orgs()).toBe("orgs");
  expect(cacheKeys.projects("acme")).toBe("projects:acme");
  expect(cacheKeys.members("acme", "Web")).toBe("members:acme/Web");
  expect(cacheKeys.workItemDetail("acme", "Web", 2003)).toBe("wi-detail:acme/Web/2003");
  expect(cacheKeys.workItemComments("acme", "Web", 2003)).toBe("wi-comments:acme/Web/2003");
  expect(cacheKeys.plansSuites("acme", "Web")).toBe("plans-suites:acme/Web");
  // New in this release, so not an earlier version's string - but pinned
  // from now on, since changing it throws away every reader's copy.
  expect(cacheKeys.suiteCases("acme", "Web", 9, 91)).toBe("suite-cases:acme/Web/9/91");
  expect(cacheKeys.runHistory("acme", "Web", 7)).toBe("run-history:acme/Web/7");
  expect(cacheKeys.points("acme", "Web", 7, 71)).toBe("points:acme/Web/7/71");
  expect(cacheKeys.boardPrs("acme", "Web")).toBe("board-prs:acme/Web");
  expect(cacheKeys.prPipeline("acme", "Web", 42, "abc")).toBe("pipe:acme/Web:42:abc");
  // NEW key, not one an earlier version stored: the old `tcm-v2-suite:`
  // seeds are deliberately abandoned, since App rebuilds a seed from
  // `plans-suites` without a request of its own.
  expect(cacheKeys.suiteSeed("acme", 42)).toBe("suite-seed:acme/42");
  // NEW key: the suggested run order read from a PBI's run-order file.
  expect(cacheKeys.runOrder("acme", "Web", 42)).toBe("run-order:acme/Web/42");
  // New key, pinned from now on.
  expect(cacheKeys.sharedStepTitle("acme", 812)).toBe("shared-step:acme/812");
});

test("cacheRemove deletes an entry that was written", () => {
  cacheWrite("k", { a: 1 });
  expect(cacheRead("k", 60_000)).toEqual({ a: 1 });
  cacheRemove("k");
  expect(cacheRead("k", 60_000)).toBeNull();
});

test("demo mode neither seeds nor stores", async () => {
  localStorage.setItem("tcm-v2-dev-demo", "on");
  const fetcher = vi.fn(async () => ["real"]);
  const opts = persistentQuery({ key: "k", fetcher, ...CACHE.structure });
  await opts.queryFn();
  expect(opts.initialData()).toBeUndefined();
  expect(localStorage.getItem("tcm-v2-cache:k")).toBeNull();
});

/**
 * One cache means one. A screen that needs cached data uses lib/cache.ts;
 * it does not reach into localStorage, seed a query by hand, invent a key
 * string, or pick a shelf life of its own. Fix a failure by using
 * persistentQuery / cacheKeys / CACHE - add the key or preset there if it
 * is new.
 */
describe("one cache", () => {
  // import.meta.url, not __dirname: this file is ESM under vitest.
  const SRC = dirname(dirname(fileURLToPath(import.meta.url)));
  const files: { file: string; text: string }[] = [];
  const walk = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name);
      if (e.isDirectory()) {
        walk(p);
        continue;
      }
      if (!/\.(tsx|ts)$/.test(e.name) || /\.test\.(tsx|ts)$/.test(e.name)) continue;
      if (e.name === "bindings.ts") continue; // generated
      const file = relative(SRC, p).replace(/\\/g, "/");
      if (file === "lib/cache.ts") continue;
      files.push({ file, text: readFileSync(p, "utf8") });
    }
  };
  walk(SRC);

  const offenders = (pattern: RegExp) => files.filter((f) => pattern.test(f.text)).map((f) => f.file);

  test("only lib/cache.ts touches the cache's storage or seeds a query from disk", () => {
    expect(offenders(/tcm-v2-cache|tcm-v2-suite:|initialDataUpdatedAt/)).toEqual([]);
  });

  test("cache keys come from cacheKeys, never a hand-written string", () => {
    // A literal passed straight in, or any string opening with one of the
    // prefixes cacheKeys owns (React Query keys are arrays - no colon).
    expect(
      offenders(
        /cache(?:Read|Write|Entry)(?:<[^>]*>)?\(\s*[`"']|persistentQuery\(\{\s*key:\s*[`"']|[`"'](?:projects|members|wi-detail|wi-comments|plans-suites|run-history|points|board-prs|pipe|suite-cases|suite-seed|shared-step):/,
      ),
    ).toEqual([]);
  });

  test("shelf lives come from CACHE, never a number at the call site", () => {
    expect(offenders(/ttlMs:\s*\d/)).toEqual([]);
  });
});

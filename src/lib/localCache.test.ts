import { afterEach, expect, test } from "vitest";
import { cacheRead, cacheWrite, cached, suspendCache } from "./localCache";

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

test("cached() fetches once, then serves from storage", async () => {
  let calls = 0;
  const fetcher = async () => {
    calls += 1;
    return ["x"];
  };
  expect(await cached("list", 60_000, fetcher)).toEqual(["x"]);
  expect(await cached("list", 60_000, fetcher)).toEqual(["x"]);
  expect(calls).toBe(1);
});

test("demo mode never reads or writes the cache", async () => {
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

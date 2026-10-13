import { afterEach, describe, expect, test } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { loadScript } from "./scriptBatch";

type Lookup = { case_id: number; script: { case_id: number; title: string } | null; error: string | null };

function serve(answer: (ids: number[]) => Lookup[]) {
  const calls: number[][] = [];
  mockIPC((cmd, args) => {
    if (cmd !== "auto_run_load_scripts") throw new Error(`unexpected ${cmd}`);
    const ids = (args as { caseIds: number[] }).caseIds;
    calls.push(ids);
    return answer(ids);
  });
  return calls;
}

const found = (id: number): Lookup => ({ case_id: id, script: { case_id: id, title: `Case ${id}` }, error: null });

afterEach(() => clearMocks());

describe("loadScript", () => {
  test("cases asked for together are read in one call", async () => {
    const calls = serve((ids) => ids.map(found));
    const got = await Promise.all([1, 2, 3].map(loadScript));
    expect(calls).toEqual([[1, 2, 3]]);
    expect(got.map((s) => s?.title)).toEqual(["Case 1", "Case 2", "Case 3"]);
  });

  test("a case with no script is null and an unreadable one fails on its own", async () => {
    serve((ids) =>
      ids.map((id) =>
        id === 2 ? { case_id: 2, script: null, error: null } : id === 3 ? { case_id: 3, script: null, error: "case-3.json is not a readable script" } : found(id),
      ),
    );
    const [one, two, three] = await Promise.allSettled([1, 2, 3].map(loadScript));
    expect(one).toMatchObject({ status: "fulfilled", value: { case_id: 1 } });
    expect(two).toEqual({ status: "fulfilled", value: null });
    expect(three).toMatchObject({ status: "rejected", reason: new Error("case-3.json is not a readable script") });
  });

  test("the same case asked twice in one batch is read once and answers both", async () => {
    const calls = serve((ids) => ids.map(found));
    const [a, b] = await Promise.all([loadScript(7), loadScript(7)]);
    expect(calls).toEqual([[7]]);
    expect(a?.case_id).toBe(7);
    expect(b?.case_id).toBe(7);
  });

  test("a large list is split so no one call carries every case", async () => {
    const calls = serve((ids) => ids.map(found));
    const ids = Array.from({ length: 450 }, (_, i) => i + 1);
    const got = await Promise.all(ids.map(loadScript));
    expect(got).toHaveLength(450);
    expect(calls.length).toBe(3);
    expect(calls.flat()).toEqual(ids);
  });

  test("a failed call fails every case it carried", async () => {
    mockIPC(() => {
      throw new Error("the app could not answer");
    });
    const settled = await Promise.allSettled([loadScript(1), loadScript(2)]);
    expect(settled.every((s) => s.status === "rejected")).toBe(true);
  });

  test("asks made later start a new batch", async () => {
    const calls = serve((ids) => ids.map(found));
    await loadScript(1);
    await loadScript(2);
    expect(calls).toEqual([[1], [2]]);
  });
});

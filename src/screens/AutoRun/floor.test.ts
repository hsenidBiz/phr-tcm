import { describe, expect, it } from "vitest";
import type { StepScript } from "../../bindings";
import { floorOf } from "./floor";

const steps = [{ action: "Save the cycle", expected: "The cycle is saved" }];

function scriptWith(kinds: object[]): StepScript[] {
  return [{ step_number: 1, actions: kinds }] as unknown as StepScript[];
}

describe("floorOf", () => {
  it("counts an api_request as a check", () => {
    const out = floorOf(steps, scriptWith([{ kind: "api_request", path: "/api/cycles/42", expect: { status: 200 } }]));
    expect(out).toEqual([{ step_number: 1, state: { kind: "checked" } }]);
  });

  it("counts an expect_response as a check", () => {
    const out = floorOf(steps, scriptWith([{ kind: "expect_response", url_contains: "/Save", status: 200 }]));
    expect(out[0].state.kind).toBe("checked");
  });

  it("still does not count driving or waiting actions", () => {
    const out = floorOf(steps, scriptWith([{ kind: "click", selector: "x" }, { kind: "sign_in", account: "a" }]));
    expect(out[0].state.kind).toBe("unchecked");
  });
});

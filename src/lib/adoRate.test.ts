import { afterEach, expect, test } from "vitest";
import { getRateLevel, RATE_LEVELS } from "./adoRate";

afterEach(() => localStorage.clear());

test("with nothing chosen, the request rate is Full speed", () => {
  expect(getRateLevel()).toBe("full");
});

test("a level someone already chose is kept", () => {
  localStorage.setItem("tcm-v2-ado-rate", "balanced");
  expect(getRateLevel()).toBe("balanced");
  localStorage.setItem("tcm-v2-ado-rate", "gentle");
  expect(getRateLevel()).toBe("gentle");
});

test("an unknown stored value falls back to the default", () => {
  localStorage.setItem("tcm-v2-ado-rate", "turbo");
  expect(getRateLevel()).toBe("full");
});

test("Full speed is listed first and no other level claims to be the recommended one", () => {
  expect(RATE_LEVELS[0].id).toBe("full");
  expect(RATE_LEVELS.filter((l) => /recommended/i.test(l.hint))).toEqual([]);
});

import { expect, test } from "vitest";
import { areaKey, sameAreaName } from "./areaName";

test("an area name's key trims, makes every run of whitespace one space, and ignores case", () => {
  for (const sp of ["\u00a0", "\u2007", "\u202f", "\t"]) {
    expect(areaKey(`${sp}Common${sp}${sp}Configurator${sp}`)).toBe("common configurator");
    expect(sameAreaName(`Manage${sp}Cycle`, " manage cycle ")).toBe(true);
  }
  expect(sameAreaName("Manage Cycle", "Manage Cycles")).toBe(false);
});

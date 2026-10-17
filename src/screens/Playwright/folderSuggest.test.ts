// Folder suggestion: which of the clone's folders a case's title points at,
// when it would otherwise go to its area's folder.

import { expect, test } from "vitest";
import { suggestFolder } from "./folderSuggest";

const AREA = "sl/admin/performance/DW-wizard-landing";
const PATHS = [
  "sl/admin/performance/DW-preview",
  "sl/admin/performance/DW-proficiency-levels",
  "sl/admin/performance/DW-proficiency-profiles",
  "sl/admin/performance/DW-rating-methods",
  "sl/admin/performance/DW-wizard-landing",
  "sl/self/leave/apply-leave",
];

test("a title that names a sibling folder gets that folder", () => {
  expect(suggestFolder("Proficiency Levels - add a level", AREA, PATHS)).toBe("sl/admin/performance/DW-proficiency-levels");
  expect(suggestFolder("Rating Methods - delete", AREA, PATHS)).toBe("sl/admin/performance/DW-rating-methods");
});

test("singular and plural count as the same word", () => {
  expect(suggestFolder("Rating Method edit", AREA, PATHS)).toBe("sl/admin/performance/DW-rating-methods");
  expect(suggestFolder("Proficiency Level - add", AREA, PATHS)).toBe("sl/admin/performance/DW-proficiency-levels");
});

test("every word of the folder name must be in the title", () => {
  expect(suggestFolder("Proficiency - open", AREA, PATHS)).toBeNull();
});

test("the area's own folder is never suggested", () => {
  expect(suggestFolder("Wizard landing shows steps", AREA, PATHS)).toBeNull();
});

test("a title that matches nothing gets nothing", () => {
  expect(suggestFolder("Create a cycle", AREA, PATHS)).toBeNull();
});

test("a tie for the most matched words suggests nothing", () => {
  const twins = [...PATHS, "sl/admin/performance/DW-proficiency-levels-old"];
  // Levels (2 words) and levels-old (3 words) differ; but two with the same count tie.
  expect(suggestFolder("Proficiency Levels - add", AREA, twins)).toBe("sl/admin/performance/DW-proficiency-levels");
  const tied = [...PATHS, "sl/admin/performance/DW-levels-proficiency"];
  expect(suggestFolder("Proficiency Levels - add", AREA, tied)).toBeNull();
});

test("the folder with the most matched words wins", () => {
  const paths = [...PATHS, "sl/admin/performance/DW-levels"];
  expect(suggestFolder("Proficiency Levels - add", AREA, paths)).toBe("sl/admin/performance/DW-proficiency-levels");
});

test("camelCase and underscores split into words", () => {
  const paths = ["sl/a/areaOne", "sl/a/rating_methods", "sl/a/otherThing"];
  expect(suggestFolder("Rating methods list", "sl/a/areaOne", paths)).toBe("sl/a/rating_methods");
  expect(suggestFolder("Other thing opens", "sl/a/areaOne", paths)).toBe("sl/a/otherThing");
});

test("folders inside the area's folder are candidates, other places are not", () => {
  const paths = ["sl/a/area", "sl/a/area/inner-thing", "sl/a/area-two/deep-thing", "sl/b/sibling-thing"];
  expect(suggestFolder("Inner thing opens", "sl/a/area", paths)).toBe("sl/a/area/inner-thing");
  expect(suggestFolder("Deep thing opens", "sl/a/area", paths)).toBeNull();
  expect(suggestFolder("Sibling thing opens", "sl/a/area", paths)).toBeNull();
});

test("words every candidate shares are ignored; one-character words are too", () => {
  // Without dropping "dw", a title naming only "dw" would match nothing useful;
  // with it, "preview" alone is enough.
  expect(suggestFolder("Preview shows the plan", AREA, PATHS)).toBe("sl/admin/performance/DW-preview");
  const paths = ["sl/a/area", "sl/a/x-alpha", "sl/a/y-beta"];
  expect(suggestFolder("Alpha opens", "sl/a/area", paths)).toBe("sl/a/x-alpha");
});

test("a trailing slash on the area's folder is ignored", () => {
  expect(suggestFolder("Rating Methods - delete", `${AREA}/`, PATHS)).toBe("sl/admin/performance/DW-rating-methods");
});

test("nothing to choose from gives nothing", () => {
  expect(suggestFolder("Rating Methods - delete", AREA, [])).toBeNull();
  expect(suggestFolder("", AREA, PATHS)).toBeNull();
});

import { expect, test } from "vitest";
import { requiredFieldsFromError } from "./adoFieldErrors";

test("extracts fields from the common ADO rule-error shapes", () => {
  expect(
    requiredFieldsFromError(
      "TF401320: Rule Error for field Remaining Work. Error code: Required, InvalidEmpty.",
    ),
  ).toEqual(["Remaining Work"]);
  expect(
    requiredFieldsFromError("VS403691: The field 'Activity' cannot be empty."),
  ).toEqual(["Activity"]);
  expect(
    requiredFieldsFromError("Field 'Microsoft.VSTS.Common.Activity' is required"),
  ).toEqual(["Activity"]);
});

test("dedupes and splits glued reference names", () => {
  const fields = requiredFieldsFromError(
    "Rule Error for field RemainingWork. Also field 'RemainingWork' cannot be empty.",
  );
  expect(fields).toEqual(["Remaining Work"]);
});

test("unknown shapes return nothing so callers fall back to the raw message", () => {
  expect(requiredFieldsFromError("TF400898: An internal error occurred.")).toEqual([]);
});

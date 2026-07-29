import { expect, test } from "vitest";
import { describeAdoError } from "./ipc";
import type { AdoError } from "../bindings";

const http = (status: number, body: string): AdoError =>
  ({ kind: "Http", detail: { status, body } }) as AdoError;

/** Azure DevOps sends its real explanation as JSON with a `message`. */
test("an Azure DevOps rule error is shown, not the status code", () => {
  expect(
    describeAdoError(http(400, JSON.stringify({ message: "TF401320: Rule Error: Start Date is required." }))),
  ).toBe("TF401320: Rule Error: Start Date is required.");
});

/**
 * The errors this APP raises about Azure DevOps - rather than receives from
 * it - are status 0 with a written sentence, and there are a lot of them:
 * a run whose outcomes could not be recorded, a test plan left orphaned, a
 * create that came back with no work item id, a board move Azure DevOps
 * refused. Every one was displayed as "Azure DevOps returned HTTP 0.", so
 * the sentence written to tell the user what to do never reached them.
 */
test("a written explanation reaches the user instead of a bare status", () => {
  const written =
    "Azure DevOps returned no result row for 2 of the 8 marked case(s). Run #12 was created but " +
    "those outcomes were NOT recorded - open it in Azure DevOps rather than marking again.";
  expect(describeAdoError(http(0, written))).toBe(written);

  const move =
    "Azure DevOps kept #12 in 'To Do' - moving to 'In Progress' is blocked by work item rules.";
  expect(describeAdoError(http(409, move))).toBe(move);
});

/** A server error PAGE is not an explanation, and neither is a wall of it. */
test("an html body or an oversized one falls back to the status", () => {
  expect(describeAdoError(http(500, "<html><body>Internal Server Error</body></html>"))).toBe(
    "Azure DevOps returned HTTP 500.",
  );
  expect(describeAdoError(http(500, "x".repeat(401)))).toBe("Azure DevOps returned HTTP 500.");
  expect(describeAdoError(http(503, "   "))).toBe("Azure DevOps returned HTTP 503.");
  expect(describeAdoError(http(503, ""))).toBe("Azure DevOps returned HTTP 503.");
});

/** The typed variants are unchanged - they never carried a body. */
test("the typed errors keep their own wording", () => {
  expect(describeAdoError({ kind: "Unauthorized" } as AdoError)).toMatch(/sign in again/i);
  expect(describeAdoError({ kind: "Forbidden" } as AdoError)).toMatch(/permission/i);
  expect(describeAdoError({ kind: "NotFound" } as AdoError)).toBe("Not found.");
  expect(
    describeAdoError({ kind: "RateLimited", detail: { retry_after_secs: 7 } } as AdoError),
  ).toContain("7s");
});

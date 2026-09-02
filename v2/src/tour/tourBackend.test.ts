import { afterEach, expect, test } from "vitest";
import { commands } from "../bindings";
import { TOUR_ORG, TOUR_PBI, TOUR_PROJECT } from "./tourData";
import { installTourBackend, restoreTourBackend, tourBackendInstalled } from "./tourBackend";

afterEach(() => restoreTourBackend());

test("installing serves the sample organisation; restoring puts the real calls back", async () => {
  const realOrgs = commands.listOrgs;
  const realCases = commands.pbiTestCasesFull;

  installTourBackend();
  expect(tourBackendInstalled()).toBe(true);

  await expect(commands.listOrgs()).resolves.toEqual({
    status: "ok",
    data: [{ name: TOUR_ORG, url: expect.any(String) }],
  });

  const cases = await commands.pbiTestCasesFull(TOUR_ORG, TOUR_PBI.id, null, null);
  expect(cases.status).toBe("ok");
  if (cases.status === "ok") expect(cases.data.length).toBeGreaterThan(2);

  restoreTourBackend();
  expect(tourBackendInstalled()).toBe(false);
  expect(commands.listOrgs).toBe(realOrgs);
  expect(commands.pbiTestCasesFull).toBe(realCases);
});

test("installing twice does not trap the stand-ins as the originals", () => {
  const realOrgs = commands.listOrgs;
  installTourBackend();
  installTourBackend();
  restoreTourBackend();
  expect(commands.listOrgs).toBe(realOrgs);
});

test("every call the toured screens make is answered locally", async () => {
  installTourBackend();
  // No mockIPC in this file: a call that fell through to the real binding
  // would reject, because vitest has no Tauri runtime behind it.
  await expect(commands.detectAiTools(null)).resolves.toHaveLength(3);
  await expect(commands.dbServerPresets()).resolves.toHaveLength(1);
  await expect(
    commands.fetchBoard(TOUR_ORG, TOUR_PROJECT, null, null, false),
  ).resolves.toMatchObject({ status: "ok" });
  await expect(commands.prOverview(TOUR_ORG, TOUR_PROJECT)).resolves.toMatchObject({
    status: "ok",
  });
  await expect(commands.listPlansWithSuites(TOUR_ORG, TOUR_PROJECT)).resolves.toMatchObject({
    status: "ok",
  });
});

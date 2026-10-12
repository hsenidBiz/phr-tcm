// Auto Run's Export to Playwright item opens the Playwright tab: the App
// shell moves there through useOpenOnExportRequest.

import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import {
  clearPlaywrightExportRequest,
  playwrightExportSnapshot,
  requestPlaywrightExport,
  useOpenOnExportRequest,
} from "./playwrightExport";

afterEach(() => clearPlaywrightExportRequest());

test("each Export request opens the tab once; clearing it opens nothing", () => {
  const open = vi.fn();
  const { rerender } = renderHook(() => useOpenOnExportRequest(open));
  expect(open).not.toHaveBeenCalled();

  act(() => requestPlaywrightExport({ pbiId: 1, caseIds: [2], modules: [[2, ""]] }));
  expect(open).toHaveBeenCalledTimes(1);
  rerender();
  expect(open).toHaveBeenCalledTimes(1);

  act(() => clearPlaywrightExportRequest());
  expect(playwrightExportSnapshot()).toBeNull();
  expect(open).toHaveBeenCalledTimes(1);

  act(() => requestPlaywrightExport({ pbiId: 1, caseIds: [2], modules: [[2, ""]] }));
  expect(open).toHaveBeenCalledTimes(2);
});

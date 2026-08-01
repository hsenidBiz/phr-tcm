import { describe, expect, it } from "vitest";

import { byteScale, formatByteProgress, formatBytes } from "./bytes";

const MB = 1024 ** 2;
const GB = 1024 ** 3;

describe("formatByteProgress", () => {
  it("scales both halves by the total, not each by itself", () => {
    // 5% of a 24.8 MB package. Scaled independently the left half would
    // read "1.2 MB" against a total in MB - fine - but at 1% it would drop
    // to KB and the pair would stop reading as one fraction.
    expect(formatByteProgress(0.05 * 24.8 * MB, 24.8 * MB)).toBe("1.2 MB of 24.8 MB");
    expect(formatByteProgress(0.01 * 24.8 * MB, 24.8 * MB)).toBe("0.2 MB of 24.8 MB");
    expect(formatByteProgress(0, 24.8 * MB)).toBe("0.0 MB of 24.8 MB");
  });

  it("says the same number on both sides when it is finished", () => {
    expect(formatByteProgress(24.8 * MB, 24.8 * MB)).toBe("24.8 MB of 24.8 MB");
  });

  it("uses the unit the total deserves", () => {
    expect(formatByteProgress(GB, 2 * GB)).toBe("1.00 GB of 2.00 GB");
    expect(formatByteProgress(400 * 1024, 800 * 1024)).toBe("400 KB of 800 KB");
    expect(formatByteProgress(100, 900)).toBe("100 B of 900 B");
  });

  it("does not invent a size it was not given", () => {
    // A feed entry with no size must not become "NaN MB" or a bar that
    // fills from nothing.
    expect(formatByteProgress(0, 0)).toBe("0 B of 0 B");
    expect(formatByteProgress(Number.NaN, Number.NaN)).toBe("0 B of 0 B");
    expect(formatBytes(-5)).toBe("0 B");
  });
});

describe("byteScale", () => {
  it("picks the largest unit the total reaches", () => {
    expect(byteScale(GB).suffix).toBe("GB");
    expect(byteScale(GB - 1).suffix).toBe("MB");
    expect(byteScale(1024).suffix).toBe("KB");
    expect(byteScale(1023).suffix).toBe("B");
  });
});

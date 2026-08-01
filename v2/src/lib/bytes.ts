/**
 * Byte sizes, written the way a download counter reads them.
 *
 * The one rule worth stating: both halves of "X of Y" are scaled by the
 * TOTAL, never each by itself. Scaling independently gives you
 * "980.0 KB of 24.8 MB" at the start of a download, which reads as two
 * unrelated numbers rather than a fraction of one thing.
 */

const UNITS = [
  { divisor: 1024 ** 3, suffix: "GB", decimals: 2 },
  { divisor: 1024 ** 2, suffix: "MB", decimals: 1 },
  { divisor: 1024, suffix: "KB", decimals: 0 },
] as const;

type Scale = { divisor: number; suffix: string; decimals: number };

/** The unit `total` should be read in. */
export function byteScale(total: number): Scale {
  if (!Number.isFinite(total) || total <= 0) return { divisor: 1, suffix: "B", decimals: 0 };
  return UNITS.find((u) => total >= u.divisor) ?? { divisor: 1, suffix: "B", decimals: 0 };
}

export function formatBytes(bytes: number, scale: Scale = byteScale(bytes)): string {
  const n = Number.isFinite(bytes) && bytes > 0 ? bytes : 0;
  return `${(n / scale.divisor).toFixed(scale.decimals)} ${scale.suffix}`;
}

/** `12.5 MB of 24.8 MB`. */
export function formatByteProgress(done: number, total: number): string {
  const scale = byteScale(total);
  return `${formatBytes(done, scale)} of ${formatBytes(total, scale)}`;
}

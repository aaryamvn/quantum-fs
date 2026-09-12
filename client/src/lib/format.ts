/** Display formatting helpers. Binary units — a vault reports what the disk reports. */

const KB = 1024;
const MB = KB * 1024;
const GB = MB * 1024;
const TB = GB * 1024;

/** One decimal, with a bare `.0` dropped so "2 GB" never reads as "2.0 GB". */
function oneDecimal(value: number): string {
  const fixed = value.toFixed(1);
  return fixed.endsWith(".0") ? fixed.slice(0, -2) : fixed;
}

/**
 * Human-readable size for a vault's footprint.
 *
 * Sub-kilobyte sizes stay in bytes (0 → "0 B"); KB and MB are whole numbers,
 * because a tenth of a megabyte is noise in a list; GB and TB carry one
 * decimal, which is where the difference starts to matter.
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  if (bytes < KB) return `${Math.round(bytes)} B`;
  if (bytes < MB) return `${Math.round(bytes / KB)} KB`;
  if (bytes < GB) return `${Math.round(bytes / MB)} MB`;
  if (bytes < TB) return `${oneDecimal(bytes / GB)} GB`;
  return `${oneDecimal(bytes / TB)} TB`;
}

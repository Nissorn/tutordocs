const KIB = 1024;
const MIB = KIB * 1024;
const GIB = MIB * 1024;

/** Formats byte counts into human sizes ("0 B", "1023 B", "37 KB", "5 MB", "1 GB"). */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  if (bytes < KIB) return `${Math.floor(bytes)} B`;
  if (bytes < MIB) return `${Math.round(bytes / KIB)} KB`;
  if (bytes < GIB) return `${Math.round(bytes / MIB)} MB`;
  return `${Math.round(bytes / GIB)} GB`;
}

const MINUTE = 60;
const HOUR = 3600;
const DAY = 86400;

function plural(n: number, unit: string): string {
  return `${n} ${unit}${n === 1 ? "" : "s"}`;
}

/** Formats an ISO 8601 string or timestamp into a relative duration ("just now", "2 minutes ago"). */
export function formatRelativeTime(isoString: string, nowMs = Date.now()): string {
  const time = Date.parse(isoString);
  if (Number.isNaN(time)) return isoString;

  const diffSeconds = Math.floor((nowMs - time) / 1000);
  if (diffSeconds < MINUTE) return "just now";
  if (diffSeconds < HOUR) return `${plural(Math.floor(diffSeconds / MINUTE), "minute")} ago`;
  if (diffSeconds < DAY) return `${plural(Math.floor(diffSeconds / HOUR), "hour")} ago`;
  return `${plural(Math.floor(diffSeconds / DAY), "day")} ago`;
}

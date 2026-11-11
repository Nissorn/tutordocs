import { describe, expect, it } from "vitest";
import { formatBytes, formatRelativeTime } from "./format";

describe("formatBytes", () => {
  it("formats zero and negative values as 0 B", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(-10)).toBe("0 B");
  });

  it("formats small byte values under 1024 B", () => {
    expect(formatBytes(1)).toBe("1 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1023)).toBe("1023 B");
  });

  it("formats kilobytes at boundaries and rounds to nearest KB", () => {
    expect(formatBytes(1024)).toBe("1 KB");
    expect(formatBytes(1535)).toBe("1 KB");
    expect(formatBytes(1536)).toBe("2 KB");
    expect(formatBytes(38022)).toBe("37 KB");
  });

  it("formats megabytes and gigabytes", () => {
    expect(formatBytes(1024 * 1024)).toBe("1 MB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5 MB");
    expect(formatBytes(1024 * 1024 * 1024)).toBe("1 GB");
  });
});

describe("formatRelativeTime", () => {
  const base = Date.parse("2026-11-10T12:00:00Z");

  it("formats times under one minute as just now", () => {
    expect(formatRelativeTime("2026-11-10T12:00:00Z", base)).toBe("just now");
    expect(formatRelativeTime("2026-11-10T11:59:30Z", base)).toBe("just now");
    expect(formatRelativeTime("2026-11-10T11:59:01Z", base)).toBe("just now");
  });

  it("formats minutes ago with singular and plural", () => {
    expect(formatRelativeTime("2026-11-10T11:59:00Z", base)).toBe("1 minute ago");
    expect(formatRelativeTime("2026-11-10T11:58:00Z", base)).toBe("2 minutes ago");
    expect(formatRelativeTime("2026-11-10T11:01:00Z", base)).toBe("59 minutes ago");
  });

  it("formats hours ago with singular and plural", () => {
    expect(formatRelativeTime("2026-11-10T11:00:00Z", base)).toBe("1 hour ago");
    expect(formatRelativeTime("2026-11-10T10:00:00Z", base)).toBe("2 hours ago");
    expect(formatRelativeTime("2026-11-09T13:00:00Z", base)).toBe("23 hours ago");
  });

  it("formats days ago with singular and plural", () => {
    expect(formatRelativeTime("2026-11-09T12:00:00Z", base)).toBe("1 day ago");
    expect(formatRelativeTime("2026-11-08T12:00:00Z", base)).toBe("2 days ago");
    expect(formatRelativeTime("2026-10-11T12:00:00Z", base)).toBe("30 days ago");
  });

  it("handles future dates as just now", () => {
    expect(formatRelativeTime("2026-11-10T12:05:00Z", base)).toBe("just now");
  });

  it("falls back to original string for invalid date", () => {
    expect(formatRelativeTime("not-a-date", base)).toBe("not-a-date");
  });
});

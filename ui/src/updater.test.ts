import { describe, expect, it, vi, beforeEach } from "vitest";
import { checkForUpdates } from "./updater";

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: vi.fn(),
  message: vi.fn(),
}));

import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ask, message } from "@tauri-apps/plugin-dialog";

describe("updater", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("checks for updates and prompts when an update is found, then downloads and relaunches on confirm", async () => {
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    vi.mocked(check).mockResolvedValue({
      version: "1.1",
      downloadAndInstall,
    } as any);
    vi.mocked(ask).mockResolvedValue(true);

    const result = await checkForUpdates();

    expect(check).toHaveBeenCalledTimes(1);
    expect(ask).toHaveBeenCalledWith("TutorDocs 1.1 is available. Install and restart?", {
      title: "TutorDocs",
      kind: "info",
    });
    expect(downloadAndInstall).toHaveBeenCalledTimes(1);
    expect(relaunch).toHaveBeenCalledTimes(1);
    expect(result).toBe(true);
  });

  it("handles version string with leading v correctly", async () => {
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    vi.mocked(check).mockResolvedValue({
      version: "v1.1",
      downloadAndInstall,
    } as any);
    vi.mocked(ask).mockResolvedValue(true);

    await checkForUpdates();

    expect(ask).toHaveBeenCalledWith("TutorDocs 1.1 is available. Install and restart?", {
      title: "TutorDocs",
      kind: "info",
    });
  });

  it("does not install or restart when user cancels the update dialog", async () => {
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    vi.mocked(check).mockResolvedValue({
      version: "1.1",
      downloadAndInstall,
    } as any);
    vi.mocked(ask).mockResolvedValue(false);

    const result = await checkForUpdates();

    expect(check).toHaveBeenCalledTimes(1);
    expect(ask).toHaveBeenCalledTimes(1);
    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(relaunch).not.toHaveBeenCalled();
    expect(result).toBe(false);
  });

  it("silently returns false on launch check when no update is available", async () => {
    vi.mocked(check).mockResolvedValue(null);

    const result = await checkForUpdates();

    expect(check).toHaveBeenCalledTimes(1);
    expect(ask).not.toHaveBeenCalled();
    expect(message).not.toHaveBeenCalled();
    expect(result).toBe(false);
  });

  it("silently returns false on launch check when network fails", async () => {
    vi.mocked(check).mockRejectedValue(new Error("Network connection lost"));

    const result = await checkForUpdates();

    expect(check).toHaveBeenCalledTimes(1);
    expect(ask).not.toHaveBeenCalled();
    expect(message).not.toHaveBeenCalled();
    expect(result).toBe(false);
  });

  it("informs the user when up to date during an interactive check", async () => {
    vi.mocked(check).mockResolvedValue(null);

    const result = await checkForUpdates({ interactive: true });

    expect(check).toHaveBeenCalledTimes(1);
    expect(ask).not.toHaveBeenCalled();
    expect(message).toHaveBeenCalledWith("You're up to date.", {
      title: "TutorDocs",
      kind: "info",
    });
    expect(result).toBe(false);
  });

  it("informs the user of error during an interactive check failure", async () => {
    vi.mocked(check).mockRejectedValue(new Error("Failed to reach update server"));

    const result = await checkForUpdates({ interactive: true });

    expect(check).toHaveBeenCalledTimes(1);
    expect(ask).not.toHaveBeenCalled();
    expect(message).toHaveBeenCalledWith("Could not check for updates.", {
      title: "TutorDocs",
      kind: "error",
    });
    expect(result).toBe(false);
  });
});

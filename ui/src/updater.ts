import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ask, message } from "@tauri-apps/plugin-dialog";

export interface CheckUpdateOptions {
  /** If true, user is notified even when no update is available or on check failure. */
  interactive?: boolean;
}

/**
 * Checks for available updates.
 *
 * In quiet mode (default), shows a confirmation dialog only if an update is found,
 * downloading, installing and relaunching on confirmation. Network failures and
 * "no update" conditions remain silent.
 *
 * In interactive mode (`options.interactive = true`), also informs the user when
 * they are up to date or if the update check failed.
 */
export async function checkForUpdates(options: CheckUpdateOptions = {}): Promise<boolean> {
  try {
    const update = await check();
    if (update) {
      const version = update.version.replace(/^v/, "");
      const shouldUpdate = await ask(`TutorDocs ${version} is available. Install and restart?`, {
        title: "TutorDocs",
        kind: "info",
      });
      if (shouldUpdate) {
        await update.downloadAndInstall();
        await relaunch();
        return true;
      }
      return false;
    }

    if (options.interactive) {
      await message("You're up to date.", {
        title: "TutorDocs",
        kind: "info",
      });
    }
    return false;
  } catch {
    if (options.interactive) {
      await message("Could not check for updates.", {
        title: "TutorDocs",
        kind: "error",
      });
    }
    return false;
  }
}

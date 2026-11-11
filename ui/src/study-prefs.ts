// The study panel's collapse choice, remembered per viewer in localStorage.
// Storage can be missing, full or blocked, so every read and write is guarded
// and the panel works the same without it.

export type PrefStorage = Pick<Storage, "getItem" | "setItem">;

const KEY = "tutordocs.studyPanel.v1";

/** The window's localStorage, or null when the webview will not hand it over. */
export function browserStorage(): PrefStorage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function readCollapsed(storage: PrefStorage | null): boolean {
  try {
    return storage?.getItem(KEY) === "collapsed";
  } catch {
    return false;
  }
}

export function writeCollapsed(storage: PrefStorage | null, collapsed: boolean): void {
  try {
    storage?.setItem(KEY, collapsed ? "collapsed" : "open");
  } catch {
    // The choice is a convenience, so a full or blocked store costs only the memory.
  }
}

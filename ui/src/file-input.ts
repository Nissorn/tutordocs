import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

// The two ways a file reaches the app: the system picker, and a drop on the
// window. Both give absolute paths, which is what `import_files` takes.

/** Asks the user for files, or for folders. An empty list means they cancelled. */
export async function pickPaths(directory: boolean): Promise<string[]> {
  const chosen = await open({ multiple: true, directory });
  if (chosen === null) return [];
  return Array.isArray(chosen) ? chosen : [chosen];
}

export type DropEvent = { type: "over" } | { type: "leave" } | { type: "drop"; paths: string[] };

/**
 * Calls `handler` as files are dragged over, away from and dropped on the
 * window. Resolves to the way to stop listening. Rejects outside the app,
 * where there is no webview to listen on.
 */
export async function listenForDrops(handler: (event: DropEvent) => void): Promise<() => void> {
  const stop = await getCurrentWebview().onDragDropEvent((event) => {
    const p = event.payload;
    if (p.type === "enter" || p.type === "over") handler({ type: "over" });
    else if (p.type === "drop") handler({ type: "drop", paths: p.paths });
    else handler({ type: "leave" });
  });
  // The real unlisten is async although its type says void; a failure to unlisten from a closed webview is not worth surfacing.
  return () => void Promise.resolve(stop()).catch(() => undefined);
}

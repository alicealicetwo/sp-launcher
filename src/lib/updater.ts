import { check, type Update } from "@tauri-apps/plugin-updater";

/**
 * Checks the endpoint configured in `tauri.conf.json`'s `plugins.updater`.
 * Returns `null` when already current (or the endpoint is unreachable —
 * a broken update check must never block the rest of the launcher).
 */
export async function checkForUpdate(): Promise<Update | null> {
  try {
    return await check();
  } catch {
    return null;
  }
}

/**
 * Downloads and installs the update. On Windows (the only target this app
 * ships for), `downloadAndInstall` launches the new NSIS installer and exits
 * the current process itself once it's handed off successfully — the
 * installer then restarts the app, so there's nothing left to do after this
 * resolves (if it resolves at all; a successful run typically ends the
 * process from underneath the caller).
 *
 * `onProgress` gets a running byte count as chunks arrive; the total isn't
 * always known ahead of time (depends on whether the server sends
 * Content-Length), so callers should treat it as "bytes so far", not a
 * percentage.
 */
export async function installUpdate(
  update: Update,
  onProgress?: (downloaded: number, total: number | null) => void
): Promise<void> {
  let downloaded = 0;
  let total: number | null = null;

  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        total = event.data.contentLength ?? null;
        onProgress?.(0, total);
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        onProgress?.(downloaded, total);
        break;
      case "Finished":
        onProgress?.(downloaded, total);
        break;
    }
  });
}

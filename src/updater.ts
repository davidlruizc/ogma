import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

export type { Update };

export interface UpdateProgress {
  downloaded: number;
  total: number | null;
}

/**
 * Ask the release endpoint whether a newer version exists.
 * Returns null when already up to date. Throws when offline or the
 * endpoint is unreachable (e.g. the latest release is still a draft).
 */
export function checkForUpdate(): Promise<Update | null> {
  return check();
}

/**
 * Download the update, verify its signature, install it and restart.
 * On Windows the installer quits the app itself, so relaunch() is only
 * reached on macOS/Linux.
 */
export async function installUpdate(update: Update, onProgress: (p: UpdateProgress) => void): Promise<void> {
  let downloaded = 0;
  let total: number | null = null;
  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        total = event.data.contentLength ?? null;
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        onProgress({ downloaded, total });
        break;
      case "Finished":
        onProgress({ downloaded, total });
        break;
    }
  });
  await relaunch();
}

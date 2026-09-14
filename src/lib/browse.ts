import { open } from "@tauri-apps/plugin-dialog";

/**
 * Opens the OS folder picker for choosing the game's install directory.
 * Shared by the Download tab's own Browse button and the Play tab's Install
 * button, so picking a folder looks and behaves the same from either place.
 * Returns the picked path, or null if the user cancelled.
 */
export async function pickInstallFolder(): Promise<string | null> {
  const picked = await open({ directory: true, multiple: false, title: "Choose install folder" });
  return typeof picked === "string" ? picked : null;
}

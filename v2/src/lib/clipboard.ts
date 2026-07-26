/**
 * One clipboard write for the whole app. The Tauri plugin is the reliable
 * path inside the WebView (navigator.clipboard is permission-flaky there);
 * the navigator fallback keeps vitest/jsdom and the dev browser working.
 */
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

export async function copyText(text: string): Promise<void> {
  try {
    await writeText(text);
    return;
  } catch {
    // Plugin unavailable (browser dev / tests) - try the web API.
  }
  await navigator.clipboard.writeText(text);
}

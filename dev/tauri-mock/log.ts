/**
 * A stand-in for `@tauri-apps/plugin-log`: the harness prints what the app
 * would write to its log file, so Jev's decisions can be read in the console.
 */
export async function info(message: string): Promise<void> {
  console.info(message);
}

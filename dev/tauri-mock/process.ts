/**
 * See `core.ts`. A browser tab cannot relaunch itself the way the bundle can,
 * so the harness prints the restart and the page stays where it is.
 */
export async function relaunch(): Promise<void> {
  console.info("[tauri-mock] relaunched");
}

/** A tab of the Settings dialog. */
export type SettingsTab = "general" | "keys" | "about";

/**
 * Asked before the shell navigates away from the current screen. Call
 * `proceed` to let the navigation happen; never calling it cancels it.
 */
export type LeaveGuard = (proceed: () => void) => void;

/**
 * The window chrome every screen shares: the sidebar, the Settings dialog,
 * and a guard a screen can hold over leaving it.
 *
 * The guard exists because navigation is no longer only a screen's own back
 * button. The sidebar can open another book or the library from anywhere, so
 * a screen with unsaved work (the select screen's decisions) has to be asked
 * first, from outside it.
 */
export function useShell() {
  const sidebarOpen = useState("shell-sidebar-open", () => true);
  const settingsOpen = useState("shell-settings-open", () => false);
  const settingsTab = useState<SettingsTab>("shell-settings-tab", () => "general");
  const leaveGuard = useState<LeaveGuard | null>("shell-leave-guard", () => null);

  function openSettings(tab: SettingsTab = "general") {
    settingsTab.value = tab;
    settingsOpen.value = true;
  }

  function toggleSidebar() {
    sidebarOpen.value = !sidebarOpen.value;
  }

  /** Runs `next` once the current screen's guard, if any, lets it. */
  function leave(next: () => void) {
    const guard = leaveGuard.value;
    if (guard) guard(next);
    else next();
  }

  return { sidebarOpen, settingsOpen, settingsTab, leaveGuard, openSettings, toggleSidebar, leave };
}

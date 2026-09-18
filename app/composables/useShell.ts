/** A tab of the Settings dialog. */
export type SettingsTab = "general" | "keys" | "about";

/**
 * The window chrome every screen shares: the sidebar and the Settings dialog.
 *
 * There is no guard over leaving a screen. The one screen with unsaved work,
 * a draft's contact sheet, keeps it in `useAnalysisJobs`, so leaving loses
 * nothing.
 */
export function useShell() {
  const sidebarOpen = useState("shell-sidebar-open", () => true);
  const settingsOpen = useState("shell-settings-open", () => false);
  const settingsTab = useState<SettingsTab>("shell-settings-tab", () => "general");

  function openSettings(tab: SettingsTab = "general") {
    settingsTab.value = tab;
    settingsOpen.value = true;
  }

  function toggleSidebar() {
    sidebarOpen.value = !sidebarOpen.value;
  }

  return { sidebarOpen, settingsOpen, settingsTab, openSettings, toggleSidebar };
}

import { invoke } from "@tauri-apps/api/core";
import type { DeviceBinding, DevicePolicy, DiscoveredDevice, Settings, Snapshot } from "./types";

export const api = {
  snapshot: () => invoke<Snapshot>("get_snapshot"),
  setPaused: (paused: boolean) => invoke<Snapshot>("set_paused", { paused }),
  lockNow: () => invoke<void>("lock_now"),
  refreshPresence: () => invoke<Snapshot>("refresh_presence"),
  listDiscovered: (maxAge = 12) => invoke<DiscoveredDevice[]>("list_discovered", { maxAge }),
  applyDevices: (devices: DeviceBinding[], policy: DevicePolicy) =>
    invoke<Snapshot>("apply_devices", { devices, policy }),
  renameDevice: (prefix: string, address: string, customName: string | null) =>
    invoke<Snapshot>("rename_device", { prefix, address, customName }),
  setPolicy: (policy: DevicePolicy) => invoke<Snapshot>("set_policy", { policy }),
  clearDevices: () => invoke<Snapshot>("clear_devices"),
  savePassword: (password: string) => invoke<Snapshot>("save_password", { password }),
  clearPassword: () => invoke<Snapshot>("clear_password"),
  updateSettings: (settings: Settings) => invoke<Snapshot>("update_settings", { settings }),
  setAutostart: (enabled: boolean) => invoke<Snapshot>("set_autostart", { enabled }),
  installCp: () => invoke<Snapshot>("install_cp"),
  uninstallCp: () => invoke<Snapshot>("uninstall_cp"),
  hideWindow: () => invoke<void>("hide_window"),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  quit: () => invoke<void>("quit_app"),
};

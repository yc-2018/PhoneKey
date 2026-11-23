export type PresenceState = "unknown" | "near" | "away";
export type DevicePolicy = "any" | "all";
export type DeviceKind = "phone" | "ble";

export interface DeviceBinding {
  name: string;
  address: string;
  prefix: string;
  kind: DeviceKind | "";
}

export interface DeviceView {
  name: string;
  address: string;
  prefix: string;
  kind: string;
  rssi: number | null;
  smoothed: number | null;
  last_seen_age: number | null;
  present: boolean;
}

export interface Settings {
  auto_lock: boolean;
  click_unlock: boolean;
  near_rssi: number;
  far_rssi: number;
  near_confirm: number;
  away_timeout: number;
  lock_grace: number;
  unlock_delay: number;
  autostart: boolean;
  paused: boolean;
  phone_adv_enabled: boolean;
  device_policy: DevicePolicy;
  bound_devices: DeviceBinding[];
}

export interface PresenceSnapshot {
  state: PresenceState;
  rssi: number | null;
  smoothed: number | null;
  last_seen_age: number | null;
  seen_since_start: boolean;
  scanning: boolean;
  error: string | null;
  address: string | null;
  source: string | null;
  phone_adv_enabled: boolean;
  phone_bound: boolean;
  phone_name: string;
  device_count: number;
  device_policy: DevicePolicy;
  devices: DeviceView[];
}

export interface Snapshot {
  presence: PresenceSnapshot;
  settings: Settings;
  locked: boolean;
  remote: boolean;
  paused: boolean;
  account: string;
  domain: string;
  has_password: boolean;
  cp_installed: boolean;
  last_action: string;
  last_error: string;
  unlock_offered: boolean;
  lock_check: PresenceState | null;
}

export interface DiscoveredDevice {
  address: string;
  name: string;
  rssi: number;
  prefix: string;
  kind: string;
  age: number;
}

export type TabKey = "devices" | "unlock" | "rules";

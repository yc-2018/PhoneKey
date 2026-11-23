use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use log::info;
use serde::Serialize;
use uuid::Uuid;

use crate::config::{save_settings, DeviceBinding, Settings};

pub const BEACON_UUID: Uuid = Uuid::from_u128(0x6d696261_6e64_4b65_7900_000000000001);
const MANUFACTURER_TAG: &[u8] = b"MIBK";
pub const PHONE_ADV_UUID: Uuid = Uuid::from_u128(0x0000fcc0_0000_1000_8000_00805f9b34fb);
const PHONE_PREFIX_CHARS: usize = 16;
const PHONE_OFFLINE_PREFIX: &str = "0341";

pub const STATE_UNKNOWN: &str = "unknown";
pub const STATE_NEAR: &str = "near";
pub const STATE_AWAY: &str = "away";

const NEAR_SEEN_WINDOW: f64 = 6.0;
const RESET_EMA_GAP: f64 = 10.0;
const EMA_ALPHA: f64 = 0.25;
const PHONE_NEAR_SEEN_WINDOW: f64 = 15.0;
const PHONE_RESET_EMA_GAP: f64 = 25.0;
const PHONE_BIND_WINDOW: f64 = 10.0;
const PHONE_MIN_AWAY_TIMEOUT: f64 = 30.0;
const VERIFY_FRESH_WINDOW: f64 = 6.0;
const VERIFY_PHONE_FRESH_WINDOW: f64 = 16.0;
pub const VERIFY_TIMEOUT: f64 = 2.5;

#[derive(Clone, Debug)]
pub struct Advertisement {
    pub address: String,
    pub name: String,
    pub rssi: i16,
    pub service_uuids: Vec<Uuid>,
    pub service_data: Vec<(Uuid, Vec<u8>)>,
    pub manufacturer_data: Vec<Vec<u8>>,
}

#[derive(Clone, Debug)]
struct DeviceState {
    name: String,
    address: String,
    kind: String,
    rssi: Option<i32>,
    ema: Option<f64>,
    last_sample: Option<Instant>,
    last_seen: Option<Instant>,
    near_since: Option<Instant>,
    weak_since: Option<Instant>,
}

#[derive(Clone, Debug)]
struct SeenDevice {
    address: String,
    name: String,
    rssi: i32,
    prefix: String,
    kind: String,
    last_seen: Instant,
}

#[derive(Clone, Debug)]
struct Candidate {
    rssi: i32,
    name: String,
    prefix: String,
    address: String,
    _seen_at: Instant,
}

#[derive(Default)]
struct PresenceInner {
    state: String,
    device_states: HashMap<String, DeviceState>,
    seen_devices: HashMap<String, SeenDevice>,
    seen_since_start: bool,
    first_seen_at: Option<Instant>,
    bind_ready_at: Option<Instant>,
    bind_candidates: HashMap<(String, String), Candidate>,
    scanning: bool,
    error: Option<String>,
}

impl PresenceInner {
    fn new() -> Self {
        Self {
            state: STATE_UNKNOWN.to_string(),
            ..Default::default()
        }
    }
}

pub struct PresenceMonitor {
    settings: Arc<RwLock<Settings>>,
    inner: Mutex<PresenceInner>,
    generation: AtomicU64,
    stop: AtomicBool,
}

#[derive(Serialize, Clone, Debug)]
pub struct DeviceView {
    pub name: String,
    pub address: String,
    pub prefix: String,
    pub kind: String,
    pub rssi: Option<i32>,
    pub smoothed: Option<f64>,
    pub last_seen_age: Option<f64>,
    pub present: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct PresenceSnapshot {
    pub state: String,
    pub rssi: Option<i32>,
    pub smoothed: Option<f64>,
    pub last_seen_age: Option<f64>,
    pub seen_since_start: bool,
    pub scanning: bool,
    pub error: Option<String>,
    pub address: Option<String>,
    pub source: Option<String>,
    pub phone_adv_enabled: bool,
    pub phone_bound: bool,
    pub phone_name: String,
    pub device_count: usize,
    pub device_policy: String,
    pub devices: Vec<DeviceView>,
}

#[derive(Serialize, Clone, Debug)]
pub struct DiscoveredDevice {
    pub address: String,
    pub name: String,
    pub rssi: i32,
    pub prefix: String,
    pub kind: String,
    pub age: f64,
}

struct DeviceStatus {
    near_ready: bool,
    absent: bool,
}

enum Classified {
    App {
        key: String,
        address: String,
        name: String,
    },
    Device {
        key: String,
        kind: String,
        address: String,
        name: String,
    },
    Candidate {
        address: String,
        payload_hex: String,
        name: String,
    },
}

impl PresenceMonitor {
    pub fn new(settings: Arc<RwLock<Settings>>) -> Arc<Self> {
        Arc::new(Self {
            settings,
            inner: Mutex::new(PresenceInner::new()),
            generation: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn set_scanning(&self, scanning: bool, error: Option<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.scanning = scanning;
        inner.error = error;
    }

    pub fn reset(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.state = STATE_UNKNOWN.to_string();
        inner.device_states.clear();
        inner.seen_devices.clear();
        inner.seen_since_start = false;
        inner.first_seen_at = None;
        inner.bind_ready_at = None;
        inner.bind_candidates.clear();
    }

    pub fn rebind(&self) {
        self.reset();
    }

    pub fn refresh(&self) {
        self.reset();
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    fn settings_snapshot(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    fn policy(settings: &Settings) -> &str {
        if settings.device_policy == "all" {
            "all"
        } else {
            "any"
        }
    }

    pub fn handle_advertisement(&self, advertisement: Advertisement) {
        let now = Instant::now();
        let settings = self.settings_snapshot();
        self.record_discovery(&advertisement, now);
        let Some(classified) = self.classify(&advertisement, &settings) else {
            return;
        };
        match classified {
            Classified::App { key, address, name } => {
                self.update_device(&key, "app", &address, &name, advertisement.rssi, now);
            }
            Classified::Device {
                key,
                kind,
                address,
                name,
            } => {
                self.update_device(&key, &kind, &address, &name, advertisement.rssi, now);
            }
            Classified::Candidate {
                address,
                payload_hex,
                name,
            } => {
                self.collect_candidate(
                    &address,
                    &payload_hex,
                    &name,
                    advertisement.rssi,
                    now,
                );
            }
        }
    }

    fn matches_app(&self, advertisement: &Advertisement) -> bool {
        if advertisement
            .service_uuids
            .iter()
            .any(|uuid| *uuid == BEACON_UUID)
        {
            return true;
        }
        advertisement
            .manufacturer_data
            .iter()
            .any(|payload| payload.starts_with(MANUFACTURER_TAG))
    }

    fn classify(
        &self,
        advertisement: &Advertisement,
        settings: &Settings,
    ) -> Option<Classified> {
        let address = advertisement.address.to_ascii_uppercase();
        let local_name: String = advertisement.name.chars().take(40).collect();

        if self.matches_app(advertisement) {
            if settings.phone_adv_enabled && !settings.bound_devices.is_empty() {
                return None;
            }
            let identity = if address.is_empty() {
                local_name.clone()
            } else {
                address.clone()
            };
            return Some(Classified::App {
                key: format!("app:{identity}"),
                address,
                name: local_name,
            });
        }

        if !settings.phone_adv_enabled {
            return None;
        }

        let mut payload_hex = String::new();
        let mut payload_name = String::new();
        for (uuid, value) in &advertisement.service_data {
            if *uuid != PHONE_ADV_UUID || value.len() < 4 {
                continue;
            }
            payload_hex = hex_lower(value);
            payload_name = extract_name(value);
            break;
        }

        if !payload_hex.is_empty() {
            if payload_hex.starts_with(PHONE_OFFLINE_PREFIX) {
                return None;
            }
            for binding in &settings.bound_devices {
                if !binding.prefix.is_empty() && payload_hex.starts_with(&binding.prefix) {
                    return Some(Classified::Device {
                        key: binding.key(),
                        kind: "phone".to_string(),
                        address,
                        name: first_non_empty(&[
                            payload_name,
                            binding.name.clone(),
                            local_name,
                        ]),
                    });
                }
            }
            for binding in &settings.bound_devices {
                if binding.prefix.is_empty()
                    && !binding.address.is_empty()
                    && address == binding.address
                {
                    return Some(Classified::Device {
                        key: binding.key(),
                        kind: "ble".to_string(),
                        address,
                        name: first_non_empty(&[local_name, binding.name.clone()]),
                    });
                }
            }
            for binding in &settings.bound_devices {
                if !binding.prefix.is_empty() && binding.address == address {
                    return None;
                }
            }
            if settings.bound_devices.is_empty() {
                return Some(Classified::Candidate {
                    address,
                    payload_hex,
                    name: first_non_empty(&[
                        payload_name,
                        local_name,
                    ]),
                });
            }
            return None;
        }

        for binding in &settings.bound_devices {
            if binding.prefix.is_empty()
                && !binding.address.is_empty()
                && address == binding.address
            {
                return Some(Classified::Device {
                    key: binding.key(),
                    kind: "ble".to_string(),
                    address,
                    name: first_non_empty(&[local_name, binding.name.clone()]),
                });
            }
        }
        None
    }

    fn collect_candidate(
        &self,
        address: &str,
        payload_hex: &str,
        name: &str,
        rssi: i16,
        now: Instant,
    ) {
        let prefix: String = payload_hex.chars().take(PHONE_PREFIX_CHARS).collect();
        let key = (address.to_string(), prefix.clone());
        let mut inner = self.inner.lock().unwrap();
        let replace = inner
            .bind_candidates
            .get(&key)
            .map(|candidate| rssi as i32 > candidate.rssi)
            .unwrap_or(true);
        if replace {
            inner.bind_candidates.insert(
                key,
                Candidate {
                    rssi: rssi as i32,
                    name: name.to_string(),
                    prefix,
                    address: address.to_string(),
                    _seen_at: now,
                },
            );
        }
    }

    fn record_discovery(&self, advertisement: &Advertisement, now: Instant) {
        let address = advertisement.address.to_ascii_uppercase();
        if address.is_empty() {
            return;
        }
        let local_name: String = advertisement.name.chars().take(40).collect();
        let mut name = local_name;
        let mut prefix = String::new();
        let mut kind = "ble".to_string();
        for (uuid, value) in &advertisement.service_data {
            if *uuid != PHONE_ADV_UUID || value.len() < 4 {
                continue;
            }
            let payload_hex = hex_lower(value);
            if payload_hex.starts_with(PHONE_OFFLINE_PREFIX) {
                return;
            }
            prefix = payload_hex.chars().take(PHONE_PREFIX_CHARS).collect();
            kind = "phone".to_string();
            let extracted = extract_name(value);
            if !extracted.is_empty() {
                name = extracted;
            }
            break;
        }
        let mut inner = self.inner.lock().unwrap();
        inner.seen_devices.insert(
            address.clone(),
            SeenDevice {
                address,
                name,
                rssi: advertisement.rssi as i32,
                prefix,
                kind,
                last_seen: now,
            },
        );
        if inner.seen_devices.len() > 120 {
            let cutoff = now - Duration::from_secs(60);
            inner
                .seen_devices
                .retain(|_, entry| entry.last_seen >= cutoff);
        }
    }

    fn update_device(
        &self,
        key: &str,
        kind: &str,
        address: &str,
        name: &str,
        rssi: i16,
        now: Instant,
    ) {
        let mut inner = self.inner.lock().unwrap();
        let entry = inner
            .device_states
            .entry(key.to_string())
            .or_insert_with(|| DeviceState {
                name: String::new(),
                address: String::new(),
                kind: kind.to_string(),
                rssi: None,
                ema: None,
                last_sample: None,
                last_seen: None,
                near_since: None,
                weak_since: None,
            });
        if !name.is_empty() {
            entry.name = name.to_string();
        }
        if !address.is_empty() {
            entry.address = address.to_string();
        }
        entry.kind = kind.to_string();
        entry.rssi = Some(rssi as i32);
        let gap_limit = if kind == "app" {
            RESET_EMA_GAP
        } else {
            PHONE_RESET_EMA_GAP
        };
        let reset = match (entry.ema, entry.last_sample) {
            (None, _) | (_, None) => true,
            (_, Some(last_sample)) => {
                now.duration_since(last_sample).as_secs_f64() > gap_limit
            }
        };
        if reset {
            entry.ema = Some(rssi as f64);
        } else if let Some(ema) = entry.ema {
            entry.ema = Some(ema + EMA_ALPHA * (rssi as f64 - ema));
        }
        entry.last_sample = Some(now);
        entry.last_seen = Some(now);
        inner.seen_since_start = true;
        if inner.first_seen_at.is_none() {
            inner.first_seen_at = Some(now);
        }
    }

    pub fn bind_auto(&self, mut binding: DeviceBinding) -> bool {
        binding.normalize();
        if binding.key().is_empty() {
            return false;
        }
        {
            let mut settings = self.settings.write().unwrap();
            if !settings.bound_devices.is_empty() {
                return false;
            }
            settings.bound_devices = vec![binding.clone()];
            save_settings(&mut settings);
        }
        let mut inner = self.inner.lock().unwrap();
        inner.bind_candidates.clear();
        inner.bind_ready_at = None;
        info!(
            "已绑定手机系统广播：{}（{}）",
            if binding.name.is_empty() {
                "未命名"
            } else {
                &binding.name
            },
            binding.address
        );
        true
    }

    pub fn check_near(&self, max_age: Option<f64>) -> bool {
        let settings = self.settings_snapshot();
        let phone_mode = settings.phone_adv_enabled;
        let bindings = if phone_mode {
            if settings.bound_devices.is_empty() {
                return false;
            }
            settings.bound_devices.clone()
        } else {
            Vec::new()
        };
        let max_age = max_age.unwrap_or(if phone_mode {
            VERIFY_PHONE_FRESH_WINDOW
        } else {
            VERIFY_FRESH_WINDOW
        });
        let inner = self.inner.lock().unwrap();
        if !inner.scanning {
            return false;
        }
        let now = Instant::now();
        let max_age = Duration::from_secs_f64(max_age);
        if phone_mode {
            let mut present = Vec::with_capacity(bindings.len());
            for binding in &bindings {
                let is_present = inner
                    .device_states
                    .get(&binding.key())
                    .and_then(|entry| {
                        let last_seen = entry.last_seen?;
                        let ema = entry.ema?;
                        Some(now.duration_since(last_seen) <= max_age && ema >= settings.near_rssi as f64)
                    })
                    .unwrap_or(false);
                present.push(is_present);
            }
            if Self::policy(&settings) == "all" {
                return !present.is_empty() && present.iter().all(|value| *value);
            }
            return present.iter().any(|value| *value);
        }

        inner.device_states.iter().any(|(key, entry)| {
            if !key.starts_with("app:") {
                return false;
            }
            let Some(last_seen) = entry.last_seen else {
                return false;
            };
            let Some(ema) = entry.ema else {
                return false;
            };
            now.duration_since(last_seen) <= max_age && ema >= settings.near_rssi as f64
        })
    }

    pub fn verify_near(&self, timeout: f64) -> bool {
        let deadline = Instant::now() + Duration::from_secs_f64(timeout);
        loop {
            if self.check_near(None) {
                return true;
            }
            if !self.inner.lock().unwrap().scanning || Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn list_discovered(&self, max_age: f64) -> Vec<DiscoveredDevice> {
        let now = Instant::now();
        let max_age = Duration::from_secs_f64(max_age);
        let inner = self.inner.lock().unwrap();
        let mut items: Vec<DiscoveredDevice> = inner
            .seen_devices
            .values()
            .filter_map(|entry| {
                let age = now.duration_since(entry.last_seen);
                if age > max_age {
                    return None;
                }
                Some(DiscoveredDevice {
                    address: entry.address.clone(),
                    name: entry.name.clone(),
                    rssi: entry.rssi,
                    prefix: entry.prefix.clone(),
                    kind: entry.kind.clone(),
                    age: round1(age.as_secs_f64()),
                })
            })
            .collect();
        items.sort_by(|a, b| b.rssi.cmp(&a.rssi));
        items
    }

    fn device_view(
        &self,
        binding: Option<&DeviceBinding>,
        entry: Option<&DeviceState>,
        now: Instant,
        settings: &Settings,
    ) -> DeviceView {
        let name = entry
            .map(|state| state.name.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| binding.map(|value| value.name.clone()))
            .unwrap_or_default();
        let address = entry
            .map(|state| state.address.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| binding.map(|value| value.address.clone()))
            .unwrap_or_default();
        let kind = entry
            .map(|state| state.kind.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| binding.map(|value| value.kind.clone()))
            .unwrap_or_else(|| "ble".to_string());
        let rssi = entry.and_then(|state| state.rssi);
        let ema = entry.and_then(|state| state.ema);
        let age = entry.and_then(|state| state.last_seen).map(|last_seen| {
            now.saturating_duration_since(last_seen).as_secs_f64().max(0.0)
        });
        let near_window = if binding.is_some() {
            PHONE_NEAR_SEEN_WINDOW
        } else {
            NEAR_SEEN_WINDOW
        };
        let present = age
            .map(|age| age <= near_window)
            .unwrap_or(false)
            && ema
                .map(|ema| ema >= settings.near_rssi as f64)
                .unwrap_or(false);
        DeviceView {
            name: if name.is_empty() {
                "未命名设备".to_string()
            } else {
                name.chars().take(40).collect()
            },
            address,
            prefix: binding.map(|value| value.prefix.clone()).unwrap_or_default(),
            kind,
            rssi,
            smoothed: ema.map(round1),
            last_seen_age: age.map(round1),
            present,
        }
    }

    pub fn snapshot(&self) -> PresenceSnapshot {
        let settings = self.settings_snapshot();
        let inner = self.inner.lock().unwrap();
        let now = Instant::now();
        let bindings = if settings.phone_adv_enabled {
            settings.bound_devices.clone()
        } else {
            Vec::new()
        };
        let mut views = Vec::new();
        if !bindings.is_empty() {
            for binding in &bindings {
                let entry = inner.device_states.get(&binding.key());
                views.push(self.device_view(Some(binding), entry, now, &settings));
            }
        } else {
            for (key, entry) in &inner.device_states {
                if key.starts_with("app:") {
                    views.push(self.device_view(None, Some(entry), now, &settings));
                }
            }
        }

        let mut primary: Option<&DeviceView> = None;
        for view in &views {
            if view.rssi.is_none() {
                continue;
            }
            let smoothed = view.smoothed.unwrap_or(-999.0);
            let current = primary
                .map(|item| item.smoothed.unwrap_or(-999.0))
                .unwrap_or(f64::NEG_INFINITY);
            if smoothed > current {
                primary = Some(view);
            }
        }

        let names: Vec<String> = bindings
            .iter()
            .map(|binding| {
                if binding.name.is_empty() {
                    "未命名设备".to_string()
                } else {
                    binding.name.clone()
                }
            })
            .collect();
        let phone_name = if !names.is_empty() {
            names.join("、")
        } else {
            primary
                .map(|view| view.name.clone())
                .unwrap_or_default()
        };
        let source = if !bindings.is_empty() {
            primary.map(|_| "phone".to_string())
        } else if !views.is_empty() {
            Some("app".to_string())
        } else {
            None
        };

        PresenceSnapshot {
            state: inner.state.clone(),
            rssi: primary.and_then(|view| view.rssi),
            smoothed: primary.and_then(|view| view.smoothed),
            last_seen_age: primary.and_then(|view| view.last_seen_age),
            seen_since_start: inner.seen_since_start,
            scanning: inner.scanning,
            error: inner.error.clone(),
            address: primary.map(|view| view.address.clone()),
            source,
            phone_adv_enabled: settings.phone_adv_enabled,
            phone_bound: !bindings.is_empty(),
            phone_name,
            device_count: bindings.len(),
            device_policy: Self::policy(&settings).to_string(),
            devices: views,
        }
    }

    pub fn tick(&self) -> Option<DeviceBinding> {
        let settings = self.settings_snapshot();
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap();
        if !inner.scanning {
            return None;
        }

        let phone_mode = settings.phone_adv_enabled && !settings.bound_devices.is_empty();
        let (near_ready, combined_absent) = if phone_mode {
            let seen_since_start = inner.seen_since_start;
            let first_seen_at = inner.first_seen_at;
            let mut statuses = Vec::with_capacity(settings.bound_devices.len());
            for binding in &settings.bound_devices {
                let entry = inner.device_states.get_mut(&binding.key());
                statuses.push(Self::device_status(
                    entry,
                    now,
                    &settings,
                    true,
                    seen_since_start,
                    first_seen_at,
                ));
            }
            if Self::policy(&settings) == "all" {
                (
                    statuses.iter().all(|status| status.near_ready),
                    statuses.iter().any(|status| status.absent) && inner.seen_since_start,
                )
            } else {
                (
                    statuses.iter().any(|status| status.near_ready),
                    statuses.iter().all(|status| status.absent) && inner.seen_since_start,
                )
            }
        } else {
            let keys: Vec<String> = inner
                .device_states
                .keys()
                .filter(|key| key.starts_with("app:"))
                .cloned()
                .collect();
            let seen_since_start = inner.seen_since_start;
            let first_seen_at = inner.first_seen_at;
            let mut statuses = Vec::with_capacity(keys.len());
            for key in keys {
                let entry = inner.device_states.get_mut(&key);
                statuses.push(Self::device_status(
                    entry,
                    now,
                    &settings,
                    false,
                    seen_since_start,
                    first_seen_at,
                ));
            }
            (
                statuses.iter().any(|status| status.near_ready),
                !statuses.is_empty()
                    && statuses.iter().all(|status| status.absent)
                    && inner.seen_since_start,
            )
        };

        let old_state = inner.state.clone();
        let new_state = if old_state == STATE_NEAR {
            if combined_absent {
                STATE_AWAY
            } else {
                STATE_NEAR
            }
        } else if near_ready {
            STATE_NEAR
        } else if inner.seen_since_start && combined_absent {
            STATE_AWAY
        } else {
            old_state.as_str()
        };
        if new_state != old_state.as_str() {
            inner.state = new_state.to_string();
            info!("手机距离状态：{old_state} -> {new_state}");
        }

        if !settings.phone_adv_enabled || !settings.bound_devices.is_empty() {
            return None;
        }
        if inner.bind_ready_at.is_none() {
            inner.bind_ready_at = Some(now + Duration::from_secs_f64(PHONE_BIND_WINDOW));
            return None;
        }
        if now < inner.bind_ready_at.unwrap() {
            return None;
        }
        let best = inner
            .bind_candidates
            .values()
            .filter(|candidate| !candidate.name.is_empty())
            .max_by_key(|candidate| candidate.rssi)
            .cloned();
        match best {
            Some(candidate) => {
                inner.bind_ready_at = None;
                Some(DeviceBinding {
                    name: candidate.name,
                    address: candidate.address,
                    prefix: candidate.prefix,
                    kind: "phone".to_string(),
                })
            }
            None => {
                inner.bind_ready_at = Some(now + Duration::from_secs_f64(PHONE_BIND_WINDOW));
                None
            }
        }
    }

    fn device_status(
        entry: Option<&mut DeviceState>,
        now: Instant,
        settings: &Settings,
        phone_mode: bool,
        seen_since_start: bool,
        first_seen_at: Option<Instant>,
    ) -> DeviceStatus {
        let (near_window, near_confirm, away_timeout) = if phone_mode {
            (
                PHONE_NEAR_SEEN_WINDOW,
                settings.near_confirm.max(2.0),
                if settings.auto_lock {
                    settings.away_timeout.max(PHONE_MIN_AWAY_TIMEOUT)
                } else {
                    settings.away_timeout
                },
            )
        } else {
            (
                NEAR_SEEN_WINDOW,
                settings.near_confirm,
                settings.away_timeout,
            )
        };

        let Some(entry) = entry else {
            let absent = seen_since_start
                && first_seen_at
                    .map(|first| now.saturating_duration_since(first).as_secs_f64() >= away_timeout)
                    .unwrap_or(false);
            return DeviceStatus {
                near_ready: false,
                absent,
            };
        };

        let last_seen = entry.last_seen;
        let ema = entry.ema;
        let seen_age = last_seen.map(|time| now.saturating_duration_since(time).as_secs_f64());
        let recent = seen_age.map(|age| age <= near_window).unwrap_or(false);
        let present = recent && ema.map(|value| value >= settings.near_rssi as f64).unwrap_or(false);
        let weak = recent && ema.map(|value| value <= settings.far_rssi as f64).unwrap_or(false);

        if present {
            if entry.near_since.is_none() {
                entry.near_since = last_seen;
            }
        } else {
            entry.near_since = None;
        }
        if weak {
            if entry.weak_since.is_none() {
                entry.weak_since = last_seen;
            }
        } else {
            entry.weak_since = None;
        }

        let absent = !present
            && (seen_age
                .map(|age| age >= away_timeout)
                .unwrap_or(false)
                || entry
                    .weak_since
                    .map(|weak_since| now.saturating_duration_since(weak_since).as_secs_f64() >= away_timeout)
                    .unwrap_or(false));
        let near_ready = present
            && entry
                .near_since
                .map(|near_since| now.saturating_duration_since(near_since).as_secs_f64() >= near_confirm)
                .unwrap_or(false);
        DeviceStatus { near_ready, absent }
    }
}

fn hex_lower(value: &[u8]) -> String {
    let mut text = String::with_capacity(value.len() * 2);
    for byte in value {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn extract_name(payload: &[u8]) -> String {
    let mut best: Vec<u8> = Vec::new();
    let mut run: Vec<u8> = Vec::new();
    for byte in payload {
        if (0x20..=0x7e).contains(byte) {
            run.push(*byte);
        } else {
            if run.len() > best.len() {
                best = run.clone();
            }
            run.clear();
        }
    }
    if run.len() > best.len() {
        best = run;
    }
    let text = String::from_utf8_lossy(&best).trim().to_string();
    let trimmed = text.trim_matches(|ch: char| !ch.is_alphanumeric());
    if trimmed.len() >= 4 {
        trimmed.chars().take(40).collect()
    } else {
        String::new()
    }
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .find(|value| !value.is_empty())
        .cloned()
        .unwrap_or_default()
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

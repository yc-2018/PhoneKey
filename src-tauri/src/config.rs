use std::collections::HashSet;
use std::fs;

use log::{error, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::paths::settings_path;

pub const MAX_DEVICES: usize = 8;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct DeviceBinding {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_name: Option<String>,
    pub address: String,
    pub prefix: String,
    pub kind: String,
}

impl DeviceBinding {
    pub fn normalize(&mut self) {
        let name = self.name.trim();
        self.name = strip_name_noise(name).chars().take(40).collect();
        self.custom_name = self
            .custom_name
            .take()
            .map(|value| value.trim().chars().take(40).collect::<String>())
            .filter(|value| !value.is_empty());
        self.address = normalize_address(&self.address);
        self.prefix = normalize_hex(&self.prefix);
        self.kind = if !self.prefix.is_empty() {
            "phone".to_string()
        } else if !self.address.is_empty() {
            "ble".to_string()
        } else {
            String::new()
        };
    }

    pub fn key(&self) -> String {
        if !self.prefix.is_empty() {
            format!("p:{}", self.prefix)
        } else if !self.address.is_empty() {
            format!("a:{}", self.address)
        } else {
            String::new()
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub auto_lock: bool,
    pub click_unlock: bool,
    pub near_rssi: i32,
    pub far_rssi: i32,
    pub near_confirm: f64,
    pub away_timeout: f64,
    pub lock_grace: f64,
    pub unlock_delay: f64,
    pub autostart: bool,
    pub hide_mode: String,
    pub theme: String,
    pub paused: bool,
    pub phone_adv_enabled: bool,
    pub device_policy: String,
    pub bound_devices: Vec<DeviceBinding>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_lock: true,
            click_unlock: true,
            near_rssi: -72,
            far_rssi: -88,
            near_confirm: 2.0,
            away_timeout: 12.0,
            lock_grace: 3.0,
            unlock_delay: 1.5,
            autostart: false,
            hide_mode: "hidden".to_string(),
            theme: "system".to_string(),
            paused: false,
            phone_adv_enabled: true,
            device_policy: "any".to_string(),
            bound_devices: Vec::new(),
        }
    }
}

impl Settings {
    pub fn normalize(&mut self) {
        self.device_policy = if self.device_policy.eq_ignore_ascii_case("all") {
            "all".to_string()
        } else {
            "any".to_string()
        };
        self.hide_mode = if self.hide_mode.eq_ignore_ascii_case("tray") {
            "tray".to_string()
        } else {
            "hidden".to_string()
        };
        self.theme = if self.theme.eq_ignore_ascii_case("light") {
            "light".to_string()
        } else if self.theme.eq_ignore_ascii_case("dark") {
            "dark".to_string()
        } else {
            "system".to_string()
        };
        let mut devices = Vec::new();
        let mut seen = HashSet::new();
        for mut binding in std::mem::take(&mut self.bound_devices) {
            binding.normalize();
            let key = binding.key();
            if key.is_empty() || !seen.insert(key) {
                continue;
            }
            devices.push(binding);
            if devices.len() >= MAX_DEVICES {
                break;
            }
        }
        self.bound_devices = devices;
        self.near_rssi = clamp_i32(self.near_rssi, -100, -40, -72);
        self.far_rssi = clamp_i32(self.far_rssi, -110, -50, -88);
        if self.far_rssi > self.near_rssi - 4 {
            self.far_rssi = (self.near_rssi - 8).max(-110);
        }
        self.near_confirm = clamp_f64(self.near_confirm, 0.5, 10.0, 2.0);
        self.away_timeout = clamp_f64(self.away_timeout, 5.0, 120.0, 12.0);
        self.lock_grace = clamp_f64(self.lock_grace, 0.0, 60.0, 3.0);
        self.unlock_delay = clamp_f64(self.unlock_delay, 0.0, 15.0, 1.5);
    }
}

// 广播在名称前带 3 个随机字节，偶尔恰好可打印（如 "r])"），会被当作名称前缀。
pub fn strip_name_noise(text: &str) -> &str {
    let bytes = text.as_bytes();
    if bytes.len() >= 7
        && bytes[3].is_ascii_alphanumeric()
        && bytes[..3]
            .iter()
            .filter(|byte| !byte.is_ascii_alphanumeric() && **byte != b' ')
            .count()
            >= 2
    {
        return &text[3..];
    }
    text
}

fn clamp_i32(value: i32, low: i32, high: i32, _default: i32) -> i32 {
    value.clamp(low, high)
}

fn clamp_f64(value: f64, low: f64, high: f64, default: f64) -> f64 {
    if !value.is_finite() {
        default
    } else {
        value.clamp(low, high)
    }
}

pub fn normalize_hex(value: &str) -> String {
    let mut text: String = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .take(32)
        .collect();
    if text.len() % 2 == 1 {
        text.pop();
    }
    if text.len() >= 8 {
        text
    } else {
        String::new()
    }
}

pub fn normalize_address(value: &str) -> String {
    let text = value.trim().to_ascii_uppercase().replace('-', ":");
    let valid = text.split(':').count() == 6
        && text
            .split(':')
            .all(|part| part.len() == 2 && part.chars().all(|ch| ch.is_ascii_hexdigit()));
    if valid {
        text
    } else {
        String::new()
    }
}

pub fn load_settings() -> Settings {
    let path = settings_path();
    let mut settings = Settings::default();
    if !path.exists() {
        settings.normalize();
        return settings;
    }
    let raw = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            warn!("读取配置失败，使用默认值: {error}");
            settings.normalize();
            return settings;
        }
    };
    let value: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(error) => {
            warn!("解析配置失败，使用默认值: {error}");
            settings.normalize();
            return settings;
        }
    };
    if let Ok(parsed) = serde_json::from_value::<Settings>(value.clone()) {
        settings = parsed;
    }
    if settings.bound_devices.is_empty() {
        if let Some(object) = value.as_object() {
            let prefix = normalize_hex(
                object
                    .get("phone_adv_prefix")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            let address = normalize_address(
                object
                    .get("phone_adv_address")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            let name: String = object
                .get("phone_adv_name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .chars()
                .take(40)
                .collect();
            if !prefix.is_empty() || !address.is_empty() {
                settings.bound_devices = vec![DeviceBinding {
                    name,
                    custom_name: None,
                    address,
                    prefix,
                    kind: String::new(),
                }];
            }
        }
    }
    settings.normalize();
    settings
}

pub fn save_settings(settings: &mut Settings) {
    settings.normalize();
    let path = settings_path();
    let temporary = path.with_extension("json.tmp");
    let content = match serde_json::to_string_pretty(settings) {
        Ok(content) => content,
        Err(error) => {
            error!("序列化配置失败: {error}");
            return;
        }
    };
    if let Err(error) = fs::write(&temporary, content) {
        error!("写入配置失败: {error}");
        return;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        error!("替换配置文件失败: {error}");
    }
}

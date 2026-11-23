use std::env;
use std::fs;
use std::path::PathBuf;

use log::warn;

pub const APP_NAME: &str = "PhoneKey";
pub const LEGACY_APP_NAME: &str = "MiPresenceKey";
pub const APP_TITLE: &str = "PhoneKey";
const MIGRATION_MARKER: &str = ".migrated-from-mipresencekey";

fn local_app_data() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("AppData/Local")))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn data_dir() -> PathBuf {
    let path = local_app_data().join(APP_NAME);
    let _ = fs::create_dir_all(&path);
    path
}

pub fn migrate_legacy_data() -> bool {
    let legacy = local_app_data().join(LEGACY_APP_NAME);
    let target = data_dir();
    let marker = target.join(MIGRATION_MARKER);
    if marker.exists() || !legacy.is_dir() {
        return false;
    }
    if legacy.canonicalize().ok() == target.canonicalize().ok() {
        return false;
    }
    let mut migrated = false;
    for name in ["settings.json", "secret.bin"] {
        let source = legacy.join(name);
        let destination = target.join(name);
        if source.is_file() && !destination.exists() {
            match fs::copy(&source, &destination) {
                Ok(_) => migrated = true,
                Err(error) => warn!("迁移旧数据 {name} 失败: {error}"),
            }
        }
    }
    if let Err(error) = fs::write(&marker, b"") {
        warn!("写入迁移标记失败: {error}");
    }
    migrated
}

pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn secret_path() -> PathBuf {
    data_dir().join("secret.bin")
}

pub fn log_path() -> PathBuf {
    data_dir().join("app.log")
}

pub fn autostart_command() -> String {
    match env::current_exe() {
        Ok(path) => format!("\"{}\" --autostart", path.display()),
        Err(_) => "PhoneKey.exe --autostart".to_string(),
    }
}

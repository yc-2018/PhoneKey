use log::{error, warn};
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
use winreg::RegKey;

use crate::paths::autostart_command;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "PhoneKey";
const LEGACY_VALUE_NAME: &str = "MiPresenceKey";

pub fn migrate_legacy() -> bool {
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
        RUN_KEY,
        KEY_READ | KEY_SET_VALUE,
    ) else {
        return false;
    };
    if key.get_value::<String, _>(LEGACY_VALUE_NAME).is_err() {
        return false;
    }
    if key.get_value::<String, _>(VALUE_NAME).is_err() {
        let _ = key.set_value(VALUE_NAME, &autostart_command());
    }
    let _ = key.delete_value(LEGACY_VALUE_NAME);
    warn!("已迁移旧版开机启动项");
    true
}

pub fn is_enabled() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|key| key.get_value::<String, _>(VALUE_NAME))
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let result = (|| -> std::io::Result<()> {
        let key = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?.0;
        if enabled {
            key.set_value(VALUE_NAME, &autostart_command())?;
        } else {
            let _ = key.delete_value(VALUE_NAME);
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(()),
        Err(err) => {
            error!("设置开机自启失败: {err}");
            Err(format!("设置开机自启失败：{err}"))
        }
    }
}

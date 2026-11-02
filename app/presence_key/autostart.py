from __future__ import annotations

import logging
import winreg

from .paths import autostart_command

log = logging.getLogger(__name__)

RUN_KEY = r"Software\Microsoft\Windows\CurrentVersion\Run"
VALUE_NAME = "PhoneKey"
LEGACY_VALUE_NAME = "MiPresenceKey"


def migrate_legacy() -> bool:
    try:
        with winreg.OpenKey(
            winreg.HKEY_CURRENT_USER, RUN_KEY, 0, winreg.KEY_READ | winreg.KEY_SET_VALUE
        ) as key:
            try:
                winreg.QueryValueEx(key, LEGACY_VALUE_NAME)
            except FileNotFoundError:
                return False
            try:
                winreg.QueryValueEx(key, VALUE_NAME)
            except FileNotFoundError:
                winreg.SetValueEx(
                    key, VALUE_NAME, 0, winreg.REG_SZ, autostart_command()
                )
            try:
                winreg.DeleteValue(key, LEGACY_VALUE_NAME)
            except FileNotFoundError:
                pass
            return True
    except OSError as exc:
        log.warning("迁移旧开机启动项失败: %s", exc)
        return False


def is_enabled() -> bool:
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY) as key:
            value, _ = winreg.QueryValueEx(key, VALUE_NAME)
            return bool(value)
    except OSError:
        return False


def set_enabled(enabled: bool) -> None:
    try:
        with winreg.CreateKeyEx(
            winreg.HKEY_CURRENT_USER, RUN_KEY, 0, winreg.KEY_SET_VALUE
        ) as key:
            if enabled:
                winreg.SetValueEx(
                    key, VALUE_NAME, 0, winreg.REG_SZ, autostart_command()
                )
            else:
                try:
                    winreg.DeleteValue(key, VALUE_NAME)
                except FileNotFoundError:
                    pass
    except OSError as exc:
        log.error("设置开机自启失败: %s", exc)

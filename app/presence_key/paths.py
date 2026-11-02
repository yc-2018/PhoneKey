from __future__ import annotations

import logging
import os
import shutil
import sys
from pathlib import Path

log = logging.getLogger(__name__)

APP_NAME = "PhoneKey"
LEGACY_APP_NAME = "MiPresenceKey"
APP_TITLE = "随身感应钥匙"
MUTEX_NAME = r"Local\PhoneKey-{d4c1a8e6-5b2f-4a97-8c3d-6e0f1b2a7c54}"
MIGRATION_MARKER = ".migrated-from-mipresencekey"


def _local_app_data() -> Path:
    base = os.environ.get("LOCALAPPDATA")
    return Path(base) if base else Path.home() / "AppData" / "Local"


def data_dir() -> Path:
    path = _local_app_data() / APP_NAME
    path.mkdir(parents=True, exist_ok=True)
    return path


def migrate_legacy_data() -> bool:
    legacy = _local_app_data() / LEGACY_APP_NAME
    target = data_dir()
    marker = target / MIGRATION_MARKER
    if marker.exists() or not legacy.is_dir() or legacy.resolve() == target.resolve():
        return False
    migrated = False
    for name in ("settings.json", "secret.bin"):
        source = legacy / name
        destination = target / name
        if source.is_file() and not destination.exists():
            try:
                shutil.copy2(source, destination)
                migrated = True
            except OSError as exc:
                log.warning("迁移旧数据 %s 失败: %s", name, exc)
    try:
        marker.touch()
    except OSError as exc:
        log.warning("写入迁移标记失败: %s", exc)
    return migrated


def settings_path() -> Path:
    return data_dir() / "settings.json"


def secret_path() -> Path:
    return data_dir() / "secret.bin"


def log_path() -> Path:
    return data_dir() / "app.log"


def resource_dir() -> Path:
    if getattr(sys, "frozen", False):
        return Path(getattr(sys, "_MEIPASS", Path(sys.executable).parent))
    return Path(__file__).resolve().parent


def resource(*parts: str) -> Path:
    return resource_dir().joinpath(*parts)


def installer_dir() -> Path:
    if getattr(sys, "frozen", False):
        return resource_dir() / "installer"
    return Path(__file__).resolve().parents[2] / "installer"


def executable_path() -> Path:
    if getattr(sys, "frozen", False):
        return Path(sys.executable)
    return Path(sys.executable)


def autostart_command() -> str:
    if getattr(sys, "frozen", False):
        return f'"{sys.executable}" --autostart'
    pythonw = Path(sys.executable).with_name("pythonw.exe")
    entry = Path(__file__).resolve().parents[1] / "run.py"
    return f'"{pythonw}" "{entry}" --autostart'

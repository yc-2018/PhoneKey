from __future__ import annotations

import json
import logging
from dataclasses import asdict, dataclass

from .paths import settings_path

log = logging.getLogger(__name__)


@dataclass
class Settings:
    auto_lock: bool = True
    click_unlock: bool = True
    near_rssi: int = -72
    far_rssi: int = -88
    near_confirm: float = 2.0
    away_timeout: float = 12.0
    lock_grace: float = 3.0
    unlock_delay: float = 1.5
    autostart: bool = False
    paused: bool = False
    phone_adv_enabled: bool = True
    phone_adv_prefix: str = ""
    phone_adv_address: str = ""
    phone_adv_name: str = ""

    def normalize(self) -> None:
        self.auto_lock = bool(self.auto_lock)
        self.click_unlock = bool(self.click_unlock)
        self.autostart = bool(self.autostart)
        self.paused = bool(self.paused)
        self.phone_adv_enabled = bool(self.phone_adv_enabled)
        self.phone_adv_prefix = _normalize_hex(self.phone_adv_prefix)
        self.phone_adv_address = _normalize_address(self.phone_adv_address)
        self.phone_adv_name = str(self.phone_adv_name or "").strip()[:40]
        self.near_rssi = _clamp(self.near_rssi, -100, -40, -72, int)
        self.far_rssi = _clamp(self.far_rssi, -110, -50, -88, int)
        if self.far_rssi > self.near_rssi - 4:
            self.far_rssi = max(-110, self.near_rssi - 8)
        self.near_confirm = _clamp(self.near_confirm, 0.5, 10.0, 2.0, float)
        self.away_timeout = _clamp(self.away_timeout, 5.0, 120.0, 12.0, float)
        self.lock_grace = _clamp(self.lock_grace, 0.0, 60.0, 3.0, float)
        self.unlock_delay = _clamp(self.unlock_delay, 0.0, 15.0, 1.5, float)


def _clamp(value, lo, hi, default, cast):
    try:
        number = cast(value)
    except (TypeError, ValueError):
        return cast(default)
    return max(lo, min(hi, number))


def _normalize_hex(value) -> str:
    text = "".join(ch for ch in str(value or "").strip().lower() if ch in "0123456789abcdef")
    text = text[:32]
    if len(text) % 2:
        text = text[:-1]
    return text if len(text) >= 8 else ""


def _normalize_address(value) -> str:
    text = str(value or "").strip().upper().replace("-", ":")
    parts = text.split(":")
    if len(parts) == 6 and all(
        len(part) == 2 and all(ch in "0123456789ABCDEF" for ch in part)
        for part in parts
    ):
        return text
    return ""


DEFAULT_SETTINGS = Settings()


def load_settings() -> Settings:
    path = settings_path()
    settings = Settings()
    if path.exists():
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
            for key, value in raw.items():
                if hasattr(settings, key):
                    setattr(settings, key, value)
        except (OSError, ValueError) as exc:
            log.warning("读取配置失败，使用默认值: %s", exc)
    settings.normalize()
    return settings


def save_settings(settings: Settings) -> None:
    settings.normalize()
    path = settings_path()
    tmp = path.with_suffix(".json.tmp")
    try:
        tmp.write_text(
            json.dumps(asdict(settings), ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
        tmp.replace(path)
    except OSError as exc:
        log.error("保存配置失败: %s", exc)

from __future__ import annotations

import asyncio
import logging
import threading
import time
from typing import Callable

from bleak import BleakScanner

from .config import save_settings

log = logging.getLogger(__name__)

BEACON_UUID = "6d696261-6e64-4b65-7900-000000000001"
BEACON_UUID_COMPACT = BEACON_UUID.replace("-", "").lower()
MANUFACTURER_TAG = b"MIBK"
PHONE_ADV_UUID = "0000fcc0-0000-1000-8000-00805f9b34fb"
PHONE_PREFIX_CHARS = 16

STATE_UNKNOWN = "unknown"
STATE_NEAR = "near"
STATE_AWAY = "away"

NEAR_SEEN_WINDOW = 6.0
RESET_EMA_GAP = 10.0
EMA_ALPHA = 0.25
APP_PREFER_WINDOW = 10.0
PHONE_NEAR_SEEN_WINDOW = 15.0
PHONE_RESET_EMA_GAP = 25.0
PHONE_BIND_WINDOW = 10.0
PHONE_MIN_AWAY_TIMEOUT = 30.0

StateCallback = Callable[[str, str], None]


class PresenceMonitor:
    def __init__(self, settings_provider, on_change: StateCallback):
        self._settings_provider = settings_provider
        self._on_change = on_change
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

        self._state = STATE_UNKNOWN
        self._rssi: int | None = None
        self._ema: float | None = None
        self._last_sample: float | None = None
        self._last_seen: float | None = None
        self._last_app: float | None = None
        self._last_phone: float | None = None
        self._source: str | None = None
        self._app_address: str | None = None
        self._address: str | None = None
        self._seen_since_start = False
        self._near_since: float | None = None
        self._weak_since: float | None = None
        self._bind_ready_at: float | None = None
        self._bind_candidates: dict[
            tuple[str, str], tuple[int, str, str, str, float]
        ] = {}
        self._scanning = False
        self._error: str | None = None

    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        self._stop.clear()
        self._thread = threading.Thread(
            target=self._run, name="ble-presence", daemon=True
        )
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=5)
            self._thread = None

    def reset(self) -> None:
        with self._lock:
            self._state = STATE_UNKNOWN
            self._rssi = None
            self._ema = None
            self._last_sample = None
            self._last_seen = None
            self._last_app = None
            self._last_phone = None
            self._source = None
            self._app_address = None
            self._address = None
            self._seen_since_start = False
            self._near_since = None
            self._weak_since = None
            self._bind_ready_at = None
            self._bind_candidates.clear()

    def rebind(self) -> None:
        self.reset()

    def snapshot(self) -> dict:
        settings = self._settings_provider()
        with self._lock:
            now = time.monotonic()
            age = None if self._last_seen is None else max(0.0, now - self._last_seen)
            return {
                "state": self._state,
                "rssi": self._rssi,
                "smoothed": None if self._ema is None else round(self._ema, 1),
                "last_seen_age": age,
                "seen_since_start": self._seen_since_start,
                "scanning": self._scanning,
                "error": self._error,
                "address": self._address,
                "source": self._source,
                "phone_adv_enabled": settings.phone_adv_enabled,
                "phone_bound": bool(
                    settings.phone_adv_prefix or settings.phone_adv_address
                ),
                "phone_name": settings.phone_adv_name,
            }

    def _run(self) -> None:
        try:
            asyncio.run(self._main())
        except Exception:
            log.exception("BLE 扫描线程异常退出")

    async def _main(self) -> None:
        while not self._stop.is_set():
            scanner = None
            try:
                scanner = BleakScanner(detection_callback=self._on_advertisement)
                await scanner.start()
            except Exception as exc:
                self._set_scanning(False, str(exc))
                log.warning("BLE 扫描启动失败：%s", exc)
                await asyncio.sleep(5)
                continue
            self._set_scanning(True, None)
            try:
                while not self._stop.is_set():
                    await asyncio.sleep(0.5)
                    try:
                        self._tick()
                    except Exception:
                        log.exception("计算手机距离状态失败")
            finally:
                try:
                    await scanner.stop()
                except Exception as exc:
                    log.warning("停止 BLE 扫描失败：%s", exc)
                self._set_scanning(False, None)

    def _set_scanning(self, scanning: bool, error: str | None) -> None:
        with self._lock:
            self._scanning = scanning
            self._error = error

    @staticmethod
    def _matches_app(advertisement) -> bool:
        for uuid in advertisement.service_uuids or ():
            if uuid.replace("-", "").lower() == BEACON_UUID_COMPACT:
                return True
        for payload in (advertisement.manufacturer_data or {}).values():
            if payload.startswith(MANUFACTURER_TAG):
                return True
        return False

    @staticmethod
    def _extract_name(payload: bytes) -> str:
        best = b""
        run = bytearray()
        for byte in payload:
            if 0x20 <= byte <= 0x7E:
                run.append(byte)
            else:
                if len(run) > len(best):
                    best = bytes(run)
                run.clear()
        if len(run) > len(best):
            best = bytes(run)
        text = best.decode("ascii", "ignore").strip()
        start, end = 0, len(text)
        while start < end and not text[start].isalnum():
            start += 1
        while end > start and not text[end - 1].isalnum():
            end -= 1
        text = text[start:end]
        return text[:40] if len(text) >= 4 else ""

    def _classify(self, device, advertisement) -> tuple[str, str, str, str] | None:
        address = str(getattr(device, "address", "") or "").upper()
        if self._matches_app(advertisement):
            return "app", "", address, ""
        settings = self._settings_provider()
        if not settings.phone_adv_enabled:
            return None
        for uuid, value in (advertisement.service_data or {}).items():
            if uuid.lower() != PHONE_ADV_UUID:
                continue
            payload = bytes(value)
            if len(payload) < 4:
                continue
            payload_hex = payload.hex()
            name = self._extract_name(payload)
            if (
                settings.phone_adv_prefix
                and payload_hex.startswith(settings.phone_adv_prefix)
            ) or (
                settings.phone_adv_address
                and address == settings.phone_adv_address
            ):
                return "phone", payload_hex, address, name
            return "candidate", payload_hex, address, name
        return None

    def _collect_candidate(
        self, address: str, payload_hex: str, name: str, rssi: int, now: float
    ) -> None:
        prefix = payload_hex[:PHONE_PREFIX_CHARS]
        key = (address, prefix)
        with self._lock:
            old = self._bind_candidates.get(key)
            if old is None or rssi > old[0]:
                self._bind_candidates[key] = (rssi, name, prefix, address, now)

    def _bind_phone(self, address: str, prefix: str, name: str) -> None:
        if not address or not prefix:
            return
        with self._lock:
            settings = self._settings_provider()
            if settings.phone_adv_prefix or settings.phone_adv_address:
                return
            settings.phone_adv_prefix = prefix
            settings.phone_adv_address = address
            settings.phone_adv_name = name
            self._bind_candidates.clear()
            self._bind_ready_at = None
        save_settings(settings)
        log.info("已绑定手机系统广播：%s（%s）", name or "未命名", address)

    def _on_advertisement(self, device, advertisement) -> None:
        match = self._classify(device, advertisement)
        if match is None:
            return
        kind, payload_hex, address, name = match
        rssi = advertisement.rssi
        if rssi is None:
            return
        now = time.monotonic()
        if kind == "candidate":
            self._collect_candidate(address, payload_hex, name, int(rssi), now)
            with self._lock:
                app_address = self._app_address
            if app_address and address == app_address:
                self._bind_phone(address, payload_hex[:PHONE_PREFIX_CHARS], name)
            return
        with self._lock:
            self._rssi = int(rssi)
            self._seen_since_start = True
            self._last_seen = now
            gap_limit = RESET_EMA_GAP if kind == "app" else PHONE_RESET_EMA_GAP
            if (
                self._ema is None
                or self._last_sample is None
                or now - self._last_sample > gap_limit
            ):
                self._ema = float(rssi)
            else:
                self._ema += EMA_ALPHA * (rssi - self._ema)
            self._last_sample = now
            if kind == "app":
                self._last_app = now
                if address:
                    self._app_address = address
            else:
                self._last_phone = now
            self._source = kind
            if address:
                self._address = address

    def _tick(self) -> None:
        settings = self._settings_provider()
        now = time.monotonic()
        changed: tuple[str, str] | None = None
        bind: tuple[str, str, str] | None = None
        with self._lock:
            if not self._scanning:
                return

            app_age = None if self._last_app is None else now - self._last_app
            if app_age is not None and app_age <= APP_PREFER_WINDOW:
                source = "app"
            elif self._last_phone is not None:
                source = "phone"
            else:
                source = "app"

            if source == "app":
                near_window = NEAR_SEEN_WINDOW
                near_confirm = settings.near_confirm
                away_timeout = settings.away_timeout
            else:
                near_window = PHONE_NEAR_SEEN_WINDOW
                near_confirm = max(settings.near_confirm, 2.0)
                away_timeout = max(settings.away_timeout, PHONE_MIN_AWAY_TIMEOUT)

            ema = self._ema
            last_seen = self._last_seen
            seen_age = None if last_seen is None else now - last_seen
            recent = seen_age is not None and seen_age <= near_window

            if recent and ema is not None and ema >= settings.near_rssi:
                if self._near_since is None:
                    self._near_since = now
            else:
                self._near_since = None

            if recent and ema is not None and ema <= settings.far_rssi:
                if self._weak_since is None:
                    self._weak_since = now
            else:
                self._weak_since = None

            absent = self._seen_since_start and (
                last_seen is None or seen_age >= away_timeout
            )
            weak = (
                self._weak_since is not None
                and now - self._weak_since >= away_timeout
            )

            old = self._state
            new = old
            if old == STATE_NEAR:
                if absent or weak:
                    new = STATE_AWAY
            else:
                if self._near_since is not None and now - self._near_since >= near_confirm:
                    new = STATE_NEAR
                elif self._seen_since_start and (absent or weak):
                    new = STATE_AWAY

            if new != old:
                self._state = new
                if new == STATE_AWAY:
                    self._near_since = None
                changed = (old, new)

            if (
                settings.phone_adv_enabled
                and not settings.phone_adv_prefix
                and not settings.phone_adv_address
            ):
                if self._bind_ready_at is None:
                    self._bind_ready_at = now + PHONE_BIND_WINDOW
                elif now >= self._bind_ready_at:
                    named = [c for c in self._bind_candidates.values() if c[1]]
                    if named:
                        best = max(named, key=lambda item: item[0])
                        bind = (best[3], best[2], best[1])
                        self._bind_ready_at = None
                    else:
                        self._bind_ready_at = now + PHONE_BIND_WINDOW

        if bind is not None:
            self._bind_phone(*bind)
        if changed:
            log.info("手机距离状态：%s -> %s", changed[0], new)
            try:
                self._on_change(*changed)
            except Exception:
                log.exception("处理手机状态变化失败")

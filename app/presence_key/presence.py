from __future__ import annotations

import asyncio
import logging
import threading
import time
from typing import Callable

from bleak import BleakScanner

from .config import DeviceBinding, save_settings

log = logging.getLogger(__name__)

BEACON_UUID = "6d696261-6e64-4b65-7900-000000000001"
BEACON_UUID_COMPACT = BEACON_UUID.replace("-", "").lower()
MANUFACTURER_TAG = b"MIBK"
PHONE_ADV_UUID = "0000fcc0-0000-1000-8000-00805f9b34fb"
PHONE_PREFIX_CHARS = 16
PHONE_OFFLINE_PREFIX = "0341"

STATE_UNKNOWN = "unknown"
STATE_NEAR = "near"
STATE_AWAY = "away"

NEAR_SEEN_WINDOW = 6.0
RESET_EMA_GAP = 10.0
EMA_ALPHA = 0.25
PHONE_NEAR_SEEN_WINDOW = 15.0
PHONE_RESET_EMA_GAP = 25.0
PHONE_BIND_WINDOW = 10.0
PHONE_MIN_AWAY_TIMEOUT = 30.0
VERIFY_FRESH_WINDOW = 6.0
# Phones using the system broadcast advertise roughly every 15 seconds; a
# click often lands between advertisements, so phone mode accepts one full
# advertisement cycle while the app beacon stays at the tighter window.
VERIFY_PHONE_FRESH_WINDOW = 16.0
VERIFY_TIMEOUT = 2.5

StateCallback = Callable[[str, str], None]


class PresenceMonitor:
    def __init__(self, settings_provider, on_change: StateCallback):
        self._settings_provider = settings_provider
        self._on_change = on_change
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._restart = threading.Event()
        self._thread: threading.Thread | None = None

        self._state = STATE_UNKNOWN
        self._device_states: dict[str, dict] = {}
        self._seen_devices: dict[str, dict] = {}
        self._seen_since_start = False
        self._first_seen_at: float | None = None
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
            self._device_states.clear()
            self._seen_devices.clear()
            self._seen_since_start = False
            self._first_seen_at = None
            self._bind_ready_at = None
            self._bind_candidates.clear()

    def rebind(self) -> None:
        self.reset()

    def refresh(self) -> None:
        self.reset()
        self._restart.set()
        if not self._thread or not self._thread.is_alive():
            self.start()

    @staticmethod
    def _policy(settings) -> str:
        return "all" if settings.device_policy == "all" else "any"

    def check_near(self, max_age: float | None = None) -> bool:
        settings = self._settings_provider()
        phone_mode = settings.phone_adv_enabled
        if phone_mode:
            bindings = list(settings.bound_devices)
            if not bindings:
                # No key device identified yet: never treat an unrelated
                # beacon as the key.
                return False
            if max_age is None:
                max_age = VERIFY_PHONE_FRESH_WINDOW
        else:
            if max_age is None:
                max_age = VERIFY_FRESH_WINDOW

        with self._lock:
            if not self._scanning:
                return False
            now = time.monotonic()
            if phone_mode:
                present = []
                for binding in bindings:
                    entry = self._device_states.get(binding.key())
                    if entry is None or entry["last_seen"] is None:
                        present.append(False)
                        continue
                    present.append(
                        now - entry["last_seen"] <= max_age
                        and entry["ema"] is not None
                        and entry["ema"] >= settings.near_rssi
                    )
                if self._policy(settings) == "all":
                    return bool(present) and all(present)
                return any(present)

            for key, entry in self._device_states.items():
                if not key.startswith("app:"):
                    continue
                if entry["last_seen"] is None:
                    continue
                if (
                    now - entry["last_seen"] <= max_age
                    and entry["ema"] is not None
                    and entry["ema"] >= settings.near_rssi
                ):
                    return True
            return False

    def verify_near(self, timeout: float = VERIFY_TIMEOUT) -> bool:
        deadline = time.monotonic() + timeout
        while True:
            if self.check_near():
                return True
            with self._lock:
                scanning = self._scanning
            if not scanning or time.monotonic() >= deadline:
                return False
            time.sleep(0.05)

    def list_discovered(self, max_age: float = 10.0) -> list[dict]:
        now = time.monotonic()
        with self._lock:
            items = []
            for entry in self._seen_devices.values():
                age = now - entry["last_seen"]
                if age > max_age:
                    continue
                items.append(
                    {
                        "address": entry["address"],
                        "name": entry["name"],
                        "rssi": entry["rssi"],
                        "prefix": entry["prefix"],
                        "kind": entry["kind"],
                        "age": round(age, 1),
                    }
                )
        items.sort(key=lambda item: item["rssi"], reverse=True)
        return items

    def snapshot(self) -> dict:
        settings = self._settings_provider()
        with self._lock:
            now = time.monotonic()
            bindings = (
                list(settings.bound_devices) if settings.phone_adv_enabled else []
            )
            views: list[dict] = []
            if bindings:
                for binding in bindings:
                    entry = self._device_states.get(binding.key())
                    views.append(self._device_view(binding, entry, now, settings))
            else:
                for key, entry in self._device_states.items():
                    if key.startswith("app:"):
                        views.append(self._device_view(None, entry, now, settings))

            primary = None
            for view in views:
                if view["rssi"] is None:
                    continue
                if primary is None or (
                    view["smoothed"] if view["smoothed"] is not None else -999
                ) > (
                    primary["smoothed"] if primary["smoothed"] is not None else -999
                ):
                    primary = view

            if bindings:
                source = "phone" if primary is not None else None
            else:
                source = "app" if views else None

            names = [binding.name or "未命名设备" for binding in bindings]
            if names:
                phone_name = "、".join(names)
            elif primary is not None:
                phone_name = primary["name"]
            else:
                phone_name = ""

            return {
                "state": self._state,
                "rssi": None if primary is None else primary["rssi"],
                "smoothed": None if primary is None else primary["smoothed"],
                "last_seen_age": None
                if primary is None
                else primary["last_seen_age"],
                "seen_since_start": self._seen_since_start,
                "scanning": self._scanning,
                "error": self._error,
                "address": None if primary is None else primary["address"],
                "source": source,
                "phone_adv_enabled": settings.phone_adv_enabled,
                "phone_bound": bool(bindings),
                "phone_name": phone_name,
                "device_count": len(bindings),
                "device_policy": self._policy(settings),
                "devices": views,
            }

    def _device_view(
        self, binding: DeviceBinding | None, entry: dict | None, now: float, settings
    ) -> dict:
        name = (entry or {}).get("name") or (binding.name if binding else "")
        address = (entry or {}).get("address") or (
            binding.address if binding else ""
        )
        kind = (entry or {}).get("kind") or (
            binding.kind if binding else "ble"
        )
        rssi = None if entry is None else entry["rssi"]
        ema = None if entry is None else entry["ema"]
        age = (
            None
            if entry is None or entry["last_seen"] is None
            else max(0.0, now - entry["last_seen"])
        )
        near_window = (
            PHONE_NEAR_SEEN_WINDOW if binding is not None else NEAR_SEEN_WINDOW
        )
        present = (
            age is not None
            and age <= near_window
            and ema is not None
            and ema >= settings.near_rssi
        )
        return {
            "name": (name or "未命名设备")[:40],
            "address": address,
            "prefix": binding.prefix if binding else "",
            "kind": kind,
            "rssi": rssi,
            "smoothed": None if ema is None else round(ema, 1),
            "last_seen_age": None if age is None else round(age, 1),
            "present": bool(present),
        }

    def _run(self) -> None:
        try:
            asyncio.run(self._main())
        except Exception:
            log.exception("BLE 扫描线程异常退出")
        finally:
            self._set_scanning(False, None)
            log.info("BLE 扫描线程已结束")

    async def _main(self) -> None:
        while not self._stop.is_set():
            self._restart.clear()
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
            log.info("BLE 扫描已启动")
            try:
                while not self._stop.is_set() and not self._restart.is_set():
                    await asyncio.sleep(0.5)
                    try:
                        self._tick()
                    except Exception:
                        log.exception("计算手机距离状态失败")
            except asyncio.CancelledError:
                log.warning("BLE 扫描被取消")
                raise
            except Exception:
                log.exception("BLE 扫描异常，准备重启")
                await asyncio.sleep(1)
            finally:
                try:
                    await scanner.stop()
                except Exception as exc:
                    log.warning("停止 BLE 扫描失败：%s", exc)
                self._set_scanning(False, None)
                log.info("BLE 扫描已停止")
            if self._restart.is_set() and not self._stop.is_set():
                log.info("正在重新启动 BLE 扫描")

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

    def _classify(self, device, advertisement) -> tuple[str, str, str, str, str] | None:
        address = str(getattr(device, "address", "") or "").upper()
        local_name = str(
            getattr(advertisement, "local_name", "")
            or getattr(device, "name", "")
            or ""
        )[:40]
        settings = self._settings_provider()
        if self._matches_app(advertisement):
            if settings.phone_adv_enabled and settings.bound_devices:
                # An explicit device list takes precedence over the legacy
                # app beacon so "all devices" cannot be bypassed.
                return None
            key = "app:" + (address or local_name)
            return "app", "", address, local_name, key
        if not settings.phone_adv_enabled:
            return None

        payload_hex = ""
        payload_name = ""
        for uuid, value in (getattr(advertisement, "service_data", None) or {}).items():
            if uuid.lower() != PHONE_ADV_UUID:
                continue
            payload = bytes(value)
            if len(payload) < 4:
                continue
            payload_hex = payload.hex()
            payload_name = self._extract_name(payload)
            break

        bindings = settings.bound_devices
        if payload_hex:
            if payload_hex.startswith(PHONE_OFFLINE_PREFIX):
                # Phones keep advertising offline-finding frames after
                # Bluetooth is turned off; they must not count as presence.
                return None
            for binding in bindings:
                if binding.prefix and payload_hex.startswith(binding.prefix):
                    return (
                        "phone",
                        payload_hex,
                        address,
                        payload_name or binding.name or local_name,
                        binding.key(),
                    )
            for binding in bindings:
                if (
                    not binding.prefix
                    and binding.address
                    and address == binding.address
                ):
                    return (
                        "ble",
                        payload_hex,
                        address,
                        local_name or binding.name,
                        binding.key(),
                    )
            for binding in bindings:
                if binding.prefix and binding.address == address:
                    # Phones can keep advertising a different frame (e.g.
                    # offline finding); only the bound frame counts.
                    return None
            if not bindings:
                return (
                    "candidate",
                    payload_hex,
                    address,
                    payload_name or local_name,
                    "",
                )
            return None

        for binding in bindings:
            if (
                not binding.prefix
                and binding.address
                and address == binding.address
            ):
                return (
                    "ble",
                    "",
                    address,
                    local_name or binding.name,
                    binding.key(),
                )
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
        binding = DeviceBinding(name=name, address=address, prefix=prefix)
        binding.normalize()
        with self._lock:
            settings = self._settings_provider()
            if settings.bound_devices:
                return
            settings.bound_devices = [binding]
            self._bind_candidates.clear()
            self._bind_ready_at = None
        save_settings(settings)
        log.info("已绑定手机系统广播：%s（%s）", name or "未命名", address)

    def _record_discovery(self, device, advertisement, now: float) -> None:
        address = str(getattr(device, "address", "") or "").upper()
        rssi = advertisement.rssi
        if not address or rssi is None:
            return
        local_name = str(
            getattr(advertisement, "local_name", "")
            or getattr(device, "name", "")
            or ""
        )[:40]
        name = local_name
        prefix = ""
        kind = "ble"
        for uuid, value in (getattr(advertisement, "service_data", None) or {}).items():
            if uuid.lower() != PHONE_ADV_UUID:
                continue
            payload = bytes(value)
            if len(payload) < 4:
                continue
            payload_hex = payload.hex()
            if payload_hex.startswith(PHONE_OFFLINE_PREFIX):
                return
            prefix = payload_hex[:PHONE_PREFIX_CHARS]
            kind = "phone"
            name = self._extract_name(payload) or local_name
            break
        with self._lock:
            self._seen_devices[address] = {
                "address": address,
                "name": name,
                "rssi": int(rssi),
                "prefix": prefix,
                "kind": kind,
                "last_seen": now,
            }
            if len(self._seen_devices) > 120:
                cutoff = now - 60
                stale = [
                    addr
                    for addr, entry in self._seen_devices.items()
                    if entry["last_seen"] < cutoff
                ]
                for addr in stale:
                    self._seen_devices.pop(addr, None)

    def _update_device(
        self,
        key: str,
        kind: str,
        address: str,
        name: str,
        rssi: int,
        now: float,
    ) -> None:
        with self._lock:
            entry = self._device_states.get(key)
            if entry is None:
                entry = {
                    "name": "",
                    "address": "",
                    "kind": kind,
                    "rssi": None,
                    "ema": None,
                    "last_sample": None,
                    "last_seen": None,
                    "near_since": None,
                    "weak_since": None,
                }
                self._device_states[key] = entry
            if name:
                entry["name"] = name
            if address:
                entry["address"] = address
            entry["kind"] = kind
            entry["rssi"] = rssi
            gap_limit = RESET_EMA_GAP if kind == "app" else PHONE_RESET_EMA_GAP
            if (
                entry["ema"] is None
                or entry["last_sample"] is None
                or now - entry["last_sample"] > gap_limit
            ):
                entry["ema"] = float(rssi)
            else:
                entry["ema"] += EMA_ALPHA * (rssi - entry["ema"])
            entry["last_sample"] = now
            entry["last_seen"] = now
            self._seen_since_start = True
            if self._first_seen_at is None:
                self._first_seen_at = now

    def _on_advertisement(self, device, advertisement) -> None:
        rssi = advertisement.rssi
        if rssi is None:
            return
        now = time.monotonic()
        self._record_discovery(device, advertisement, now)
        match = self._classify(device, advertisement)
        if match is None:
            return
        kind, payload_hex, address, name, key = match
        if kind == "candidate":
            self._collect_candidate(address, payload_hex, name, int(rssi), now)
            return
        self._update_device(key, kind, address, name, int(rssi), now)

    def _device_status(
        self, entry: dict | None, now: float, settings, phone_mode: bool
    ) -> dict:
        if phone_mode:
            near_window = PHONE_NEAR_SEEN_WINDOW
            near_confirm = max(settings.near_confirm, 2.0)
            away_timeout = (
                max(settings.away_timeout, PHONE_MIN_AWAY_TIMEOUT)
                if settings.auto_lock
                else settings.away_timeout
            )
        else:
            near_window = NEAR_SEEN_WINDOW
            near_confirm = settings.near_confirm
            away_timeout = settings.away_timeout

        if entry is None:
            absent = (
                self._seen_since_start
                and self._first_seen_at is not None
                and now - self._first_seen_at >= away_timeout
            )
            return {"present": False, "near_ready": False, "absent": absent}

        last_seen = entry["last_seen"]
        ema = entry["ema"]
        seen_age = None if last_seen is None else now - last_seen
        recent = seen_age is not None and seen_age <= near_window
        present = recent and ema is not None and ema >= settings.near_rssi
        weak = recent and ema is not None and ema <= settings.far_rssi

        if present:
            if entry["near_since"] is None:
                entry["near_since"] = last_seen
        else:
            entry["near_since"] = None
        if weak:
            if entry["weak_since"] is None:
                entry["weak_since"] = last_seen
        else:
            entry["weak_since"] = None

        absent = (
            (seen_age is not None and seen_age >= away_timeout)
            or (
                entry["weak_since"] is not None
                and now - entry["weak_since"] >= away_timeout
            )
            or (
                last_seen is None
                and self._seen_since_start
                and self._first_seen_at is not None
                and now - self._first_seen_at >= away_timeout
            )
        )
        near_ready = (
            present
            and entry["near_since"] is not None
            and now - entry["near_since"] >= near_confirm
        )
        return {
            "present": present,
            "near_ready": near_ready,
            "absent": absent,
        }

    def _tick(self) -> None:
        settings = self._settings_provider()
        now = time.monotonic()
        changed: tuple[str, str] | None = None
        bind: tuple[str, str, str] | None = None
        with self._lock:
            if not self._scanning:
                return

            if settings.phone_adv_enabled and settings.bound_devices:
                bindings = list(settings.bound_devices)
                statuses = [
                    self._device_status(
                        self._device_states.get(binding.key()),
                        now,
                        settings,
                        True,
                    )
                    for binding in bindings
                ]
                if self._policy(settings) == "all":
                    near_ready = all(s["near_ready"] for s in statuses)
                    combined_absent = (
                        any(s["absent"] for s in statuses)
                        and self._seen_since_start
                    )
                else:
                    near_ready = any(s["near_ready"] for s in statuses)
                    combined_absent = (
                        all(s["absent"] for s in statuses)
                        and self._seen_since_start
                    )
            else:
                entries = [
                    entry
                    for key, entry in self._device_states.items()
                    if key.startswith("app:")
                ]
                statuses = [
                    self._device_status(entry, now, settings, False)
                    for entry in entries
                ]
                near_ready = any(s["near_ready"] for s in statuses)
                combined_absent = (
                    bool(statuses)
                    and all(s["absent"] for s in statuses)
                    and self._seen_since_start
                )

            old = self._state
            new = old
            if old == STATE_NEAR:
                if combined_absent:
                    new = STATE_AWAY
            else:
                if near_ready:
                    new = STATE_NEAR
                elif self._seen_since_start and combined_absent:
                    new = STATE_AWAY

            if new != old:
                self._state = new
                changed = (old, new)

            if settings.phone_adv_enabled and not settings.bound_devices:
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

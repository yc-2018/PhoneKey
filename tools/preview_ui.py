"""Render the current UI with demo data and save README screenshots."""
import ctypes
import sys
import time
from pathlib import Path
from unittest.mock import patch

from PIL import ImageGrab

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "app"))

import presence_key.ui as ui_module  # noqa: E402
from presence_key.config import DeviceBinding, Settings  # noqa: E402
from presence_key.controller import Controller  # noqa: E402
from presence_key.presence import STATE_NEAR  # noqa: E402
from presence_key.ui import AdvancedDialog, DevicePickerDialog, MainWindow  # noqa: E402

u = ctypes.windll.user32
u.SetThreadDpiAwarenessContext(ctypes.c_void_p(-4))

OUT = ROOT / "docs" / "images"
OUT.mkdir(parents=True, exist_ok=True)


def pump(seconds=0.6):
    end = time.time() + seconds
    while time.time() < end:
        app.update()
        time.sleep(0.02)


def capture(win, name):
    hwnd = u.GetAncestor(win.winfo_id(), 2)
    u.ShowWindow(hwnd, 9)
    u.SetForegroundWindow(hwnd)
    pump(0.5)
    img = ImageGrab.grab(window=hwnd)
    out = OUT / name
    img.save(out)
    print("saved", out, img.size)


DEMO_DEVICES = [
    {
        "name": "我的手机",
        "address": "AA:BB:CC:DD:EE:FF",
        "prefix": "0011223344556677",
        "kind": "phone",
        "rssi": -57,
        "smoothed": -57.8,
        "last_seen_age": 1.2,
        "present": True,
    },
    {
        "name": "小米手环 9 Pro",
        "address": "AA:BB:CC:DD:EE:BD",
        "prefix": "",
        "kind": "ble",
        "rssi": -63,
        "smoothed": -63.4,
        "last_seen_age": 3.6,
        "present": True,
    },
]


def demo_snapshot():
    return {
        "presence": {
            "state": STATE_NEAR,
            "rssi": -57,
            "smoothed": -57.8,
            "last_seen_age": 1.2,
            "seen_since_start": True,
            "scanning": True,
            "error": None,
            "address": "AA:BB:CC:DD:EE:FF",
            "source": "phone",
            "phone_adv_enabled": True,
            "phone_bound": True,
            "phone_name": "我的手机、小米手环 9 Pro",
            "device_count": 2,
            "device_policy": "any",
            "devices": DEMO_DEVICES,
        },
        "locked": True,
        "remote": False,
        "paused": False,
        "has_password": True,
        "cp_installed": True,
        "last_action": "已提供手机解锁选项 · 点击时检测手机",
        "last_error": "",
        "unlock_offered": True,
        "lock_check": STATE_NEAR,
    }


settings = Settings(
    auto_lock=False,
    click_unlock=True,
    autostart=True,
    paused=False,
    device_policy="any",
    bound_devices=[
        DeviceBinding(
            name="我的手机",
            address="AA:BB:CC:DD:EE:FF",
            prefix="0011223344556677",
        ),
        DeviceBinding(name="小米手环 9 Pro", address="AA:BB:CC:DD:EE:BD"),
    ],
)
controller = Controller(settings)
controller.snapshot = demo_snapshot
controller.list_discovered_devices = lambda max_age=10.0: [
    {
        "address": "AA:BB:CC:DD:EE:FF",
        "name": "我的手机",
        "prefix": "0011223344556677",
        "kind": "phone",
        "rssi": -57,
        "age": 1.2,
    },
    {
        "address": "AA:BB:CC:DD:EE:BD",
        "name": "小米手环 9 Pro",
        "prefix": "",
        "kind": "ble",
        "rssi": -63,
        "age": 3.6,
    },
    {
        "address": "AA:BB:CC:DD:EE:C1",
        "name": "客厅音箱",
        "prefix": "",
        "kind": "ble",
        "rssi": -79,
        "age": 8.8,
    },
]

with patch.object(ui_module.credential, "account_name", return_value="示例账户"):
    app = MainWindow(controller, on_hide=lambda: None)
    app.geometry("620x840+40+0")
    pump(0.8)

    capture(app, "status.png")
    app._tabs.set("解锁设置")
    pump(0.8)
    capture(app, "unlock-settings.png")

    dialog = AdvancedDialog(app, controller, lambda: None)
    dialog.geometry("560x780+100+20")
    pump(0.6)
    capture(dialog, "advanced-settings.png")
    dialog.destroy()

    with patch.object(DevicePickerDialog, "_start_scan", lambda self: None):
        picker = DevicePickerDialog(app, controller, lambda: None)
        picker.geometry("600x720+100+10")
        picker._merge_discovered()
        picker._status.configure(text="已列出 3 台设备，勾选后点击确定。")
        picker._render()
        pump(0.6)
        capture(picker, "device-picker.png")
        picker.destroy()

    app.destroy()

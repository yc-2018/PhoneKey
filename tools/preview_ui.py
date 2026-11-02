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
from presence_key.config import Settings  # noqa: E402
from presence_key.controller import Controller  # noqa: E402
from presence_key.presence import STATE_NEAR  # noqa: E402
from presence_key.ui import AdvancedDialog, MainWindow  # noqa: E402

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
            "phone_name": "我的手机",
        },
        "locked": True,
        "remote": False,
        "paused": False,
        "has_password": True,
        "cp_installed": True,
        "last_action": "手机在附近 · 请在锁屏界面点击解锁",
        "last_error": "",
        "unlock_offered": True,
    }


settings = Settings(
    auto_lock=False,
    click_unlock=True,
    autostart=True,
    paused=False,
    phone_adv_prefix="0011223344556677",
    phone_adv_address="AA:BB:CC:DD:EE:FF",
    phone_adv_name="我的手机",
)
controller = Controller(settings)
controller.snapshot = demo_snapshot

with patch.object(ui_module.credential, "account_name", return_value="示例账户"):
    app = MainWindow(controller, on_hide=lambda: None)
    app.geometry("620x840+40+0")
    pump(0.8)

    capture(app, "status.png")
    app._tabs.set("解锁设置")
    pump(0.8)
    capture(app, "unlock-settings.png")

    dialog = AdvancedDialog(app, settings, lambda: None, lambda: None)
    dialog.geometry("560x620+100+20")
    pump(0.6)
    capture(dialog, "advanced-settings.png")
    dialog.destroy()

    app.destroy()

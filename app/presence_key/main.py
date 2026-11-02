from __future__ import annotations

import ctypes
import logging
import logging.handlers
import queue
import sys

import customtkinter as ctk

from . import __version__, autostart
from .config import load_settings
from .controller import Controller
from .paths import APP_TITLE, MUTEX_NAME, log_path, migrate_legacy_data
from .tray import Tray
from .ui import MainWindow

log = logging.getLogger(__name__)

ERROR_ALREADY_EXISTS = 183


def _setup_logging() -> None:
    formatter = logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s")
    file_handler = logging.handlers.RotatingFileHandler(
        log_path(), maxBytes=1_000_000, backupCount=2, encoding="utf-8"
    )
    file_handler.setFormatter(formatter)
    root = logging.getLogger()
    root.setLevel(logging.INFO)
    root.addHandler(file_handler)
    if sys.stderr is not None:
        stream = logging.StreamHandler()
        stream.setFormatter(formatter)
        root.addHandler(stream)

    def handle_exception(exc_type, exc_value, exc_tb):
        log.error("未捕获异常", exc_info=(exc_type, exc_value, exc_tb))

    sys.excepthook = handle_exception


def _acquire_mutex():
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    handle = kernel32.CreateMutexW(None, False, MUTEX_NAME)
    if ctypes.get_last_error() == ERROR_ALREADY_EXISTS:
        return None
    return handle


def _show_existing_window() -> bool:
    user32 = ctypes.windll.user32
    hwnd = user32.FindWindowW(None, APP_TITLE)
    if not hwnd:
        return False
    user32.ShowWindow(hwnd, 9)
    user32.SetForegroundWindow(hwnd)
    return True


class Application:
    def __init__(self, settings, start_hidden: bool):
        self.controller = Controller(settings)
        self.window = MainWindow(self.controller, on_hide=self._on_hidden)
        self.tray = Tray(self)
        self._start_hidden = start_hidden
        self._closing = False

    def run(self) -> None:
        self.controller.start()
        self.tray.start()
        if self._start_hidden:
            self.window.withdraw()
        self.window.mainloop()

    def ui_call(self, fn, *args) -> None:
        self.window.post(fn, *args)

    def show_window(self) -> None:
        self.window.deiconify()
        self.window.lift()
        self.window.focus_force()

    def toggle_pause(self) -> None:
        self.controller.set_paused(not self.controller.settings.paused)
        self.window.refresh_now()

    def _on_hidden(self) -> None:
        self.tray.notify("程序仍在后台运行，可从托盘图标打开设置。")

    def quit(self) -> None:
        if self._closing:
            return
        self._closing = True
        log.info("正在退出")
        self.controller.stop()
        self.tray.stop()
        try:
            self.window.destroy()
        except Exception:
            pass


def main() -> None:
    mutex = _acquire_mutex()
    if mutex is None:
        if not _show_existing_window():
            ctypes.windll.user32.MessageBoxW(
                0, "随身感应钥匙已在运行，请查看系统托盘。", APP_TITLE, 0x40
            )
        return

    _setup_logging()
    if migrate_legacy_data():
        log.info("已从旧版数据目录迁移配置和密码")
    if autostart.migrate_legacy():
        log.info("已迁移旧版开机启动项")
    settings = load_settings()
    autostart.set_enabled(autostart.is_enabled())
    settings.autostart = autostart.is_enabled()

    ctk.set_appearance_mode("system")
    ctk.set_default_color_theme("blue")

    app = Application(settings, start_hidden="--autostart" in sys.argv)
    log.info("%s 启动 v%s", APP_TITLE, __version__)
    app.run()
    log.info("程序已退出")

from __future__ import annotations

import logging
import threading

import pystray
from PIL import Image

from .paths import APP_NAME, APP_TITLE, resource

log = logging.getLogger(__name__)


class Tray:
    def __init__(self, app):
        self._app = app
        image = Image.open(resource("resources", "icon.png"))
        menu = pystray.Menu(
            pystray.MenuItem(
                "打开设置", lambda: app.ui_call(app.show_window), default=True
            ),
            pystray.MenuItem(
                "暂停自动操作",
                lambda: app.ui_call(app.toggle_pause),
                checked=lambda item: app.controller.settings.paused,
            ),
            pystray.MenuItem("立即锁屏", lambda: app.ui_call(app.controller.lock_now)),
            pystray.Menu.SEPARATOR,
            pystray.MenuItem("退出", lambda: app.ui_call(app.quit)),
        )
        self._icon = pystray.Icon(APP_NAME, image, APP_TITLE, menu)
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        self._thread = threading.Thread(target=self._icon.run, name="tray", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        try:
            self._icon.stop()
        except Exception:
            log.exception("停止托盘图标失败")

    def notify(self, message: str) -> None:
        try:
            self._icon.notify(message, APP_TITLE)
        except Exception:
            pass

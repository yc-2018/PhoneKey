from __future__ import annotations

import logging
import queue
import threading
import time

from . import credential, lockstate, secret, unlock
from .config import Settings, save_settings
from .presence import STATE_AWAY, STATE_NEAR, STATE_UNKNOWN, PresenceMonitor

log = logging.getLogger(__name__)

CP_CACHE_SECONDS = 5.0
OFFER_RETRY_SECONDS = 2.0
OFFER_CONNECT_TIMEOUT = 0.8

PRESENCE_TEXT = {
    STATE_NEAR: "在附近",
    STATE_AWAY: "已离开",
    STATE_UNKNOWN: "未知",
}


class Controller:
    def __init__(self, settings: Settings):
        self.settings = settings
        self._presence = PresenceMonitor(lambda: self.settings, self._on_presence_change)
        self._events: queue.Queue[tuple[str, bool | str]] = queue.Queue()
        self._stop = threading.Event()
        self._threads: list[threading.Thread] = []
        self._lock = threading.RLock()

        self._locked = lockstate.is_locked()
        self._remote = lockstate.is_remote_session()
        self._presence_state = STATE_UNKNOWN
        self._pending_lock_at: float | None = None
        self._offer_at: float | None = None
        self._offered = False
        self._offer_failures = 0
        self._last_action = "已就绪"
        self._last_error = ""
        self._cp_checked = 0.0
        self._cp_value = False

    def start(self) -> None:
        self._presence.start()
        for target, name in (
            (self._lock_loop, "lock-monitor"),
            (self._work_loop, "auto-controller"),
        ):
            thread = threading.Thread(target=target, name=name, daemon=True)
            thread.start()
            self._threads.append(thread)

    def stop(self) -> None:
        self._stop.set()
        self._withdraw_offer("应用退出")
        self._presence.stop()
        for thread in self._threads:
            thread.join(timeout=3)

    def set_paused(self, paused: bool) -> None:
        self.settings.paused = bool(paused)
        save_settings(self.settings)
        if paused:
            with self._lock:
                self._pending_lock_at = None
                self._offer_at = None
                self._last_action = "已暂停自动锁屏与手机解锁"
            log.info("自动操作已暂停")
        else:
            self._presence.reset()
            with self._lock:
                self._last_action = "已恢复自动操作"
                self._last_error = ""
            log.info("自动操作已恢复")

    def lock_now(self) -> None:
        if lockstate.lock_workstation():
            log.info("手动触发锁屏")
        else:
            self._set_error("锁屏失败")

    def cp_installed(self) -> bool:
        now = time.monotonic()
        with self._lock:
            if now - self._cp_checked < CP_CACHE_SECONDS:
                return self._cp_value
        value = unlock.cp_installed()
        with self._lock:
            self._cp_value = value
            self._cp_checked = now
            return value

    def invalidate_cp_cache(self) -> None:
        with self._lock:
            self._cp_checked = 0.0

    def rebind_phone(self) -> None:
        self.settings.phone_adv_enabled = True
        self.settings.phone_adv_prefix = ""
        self.settings.phone_adv_address = ""
        self.settings.phone_adv_name = ""
        save_settings(self.settings)
        self._presence.rebind()
        with self._lock:
            self._last_action = "正在重新识别手机广播…"
        log.info("已清除手机广播绑定，开始重新识别")

    def snapshot(self) -> dict:
        presence = self._presence.snapshot()
        with self._lock:
            locked = self._locked
            remote = self._remote
            action = self._last_action
            error = self._last_error
            offered = self._offered
        return {
            "presence": presence,
            "locked": locked,
            "remote": remote,
            "paused": self.settings.paused,
            "has_password": secret.has_secret(),
            "cp_installed": self.cp_installed(),
            "last_action": action,
            "last_error": error,
            "unlock_offered": offered,
        }

    def _set_error(self, message: str) -> None:
        with self._lock:
            self._last_error = message
        log.warning(message)

    def _on_presence_change(self, old: str, new: str) -> None:
        self._events.put(("presence", new))

    def _lock_loop(self) -> None:
        while not self._stop.is_set():
            try:
                locked = lockstate.is_locked()
                remote = lockstate.is_remote_session()
            except Exception:
                log.exception("读取锁屏状态失败")
                self._stop.wait(1.0)
                continue
            if locked != self._locked:
                self._events.put(("locked", locked))
            if remote != self._remote:
                self._events.put(("remote", remote))
            self._stop.wait(0.6)

    def _work_loop(self) -> None:
        while not self._stop.is_set():
            try:
                event = self._events.get(timeout=0.4)
            except queue.Empty:
                event = None
            try:
                if event is not None:
                    self._handle_event(*event)
                self._evaluate()
            except Exception:
                log.exception("自动控制逻辑异常")

    def _handle_event(self, kind: str, value: bool | str) -> None:
        with self._lock:
            if kind == "presence":
                self._presence_state = str(value)
                if value == STATE_NEAR:
                    self._pending_lock_at = None
                    if self._locked and self.settings.click_unlock:
                        self._last_action = "手机在附近 · 正在准备锁屏解锁选项"
                elif value == STATE_AWAY:
                    self._pending_lock_at = None
                    self._offer_at = None
                    if self._locked:
                        self._last_action = "手机已离开 · 锁屏解锁选项不可用"
            elif kind == "locked":
                locked = bool(value)
                if locked == self._locked:
                    return
                self._locked = locked
                self._pending_lock_at = None
                self._offer_at = None
                if locked:
                    self._last_action = "屏幕已锁定"
                    log.info(
                        "检测到锁屏（手机状态：%s）",
                        PRESENCE_TEXT.get(self._presence_state, self._presence_state),
                    )
                else:
                    self._last_action = "屏幕已解锁"
                    self._last_error = ""
                    log.info("检测到解锁")
            elif kind == "remote":
                self._remote = bool(value)
                if value:
                    self._pending_lock_at = None
                    self._offer_at = None
                    log.info("检测到远程桌面会话，自动操作暂停")
                else:
                    log.info("远程桌面会话结束")

    def _evaluate(self) -> None:
        settings = self.settings
        presence = self._presence.snapshot()
        now = time.monotonic()
        action: str | None = None

        with self._lock:
            active = not settings.paused and not self._remote
            if not active:
                self._pending_lock_at = None
                self._offer_at = None
                if self._offered:
                    action = "withdraw"
            else:
                if (
                    settings.auto_lock
                    and not self._locked
                    and presence["state"] == STATE_AWAY
                    and presence["seen_since_start"]
                ):
                    if self._pending_lock_at is None:
                        self._pending_lock_at = now + settings.lock_grace
                elif self._pending_lock_at is not None and (
                    self._locked or presence["state"] != STATE_AWAY
                ):
                    self._pending_lock_at = None

                if self._pending_lock_at is not None and now >= self._pending_lock_at:
                    self._pending_lock_at = None
                    action = "lock"

                ready = (
                    settings.click_unlock
                    and self._locked
                    and presence["state"] == STATE_NEAR
                    and secret.has_secret()
                    and self.cp_installed()
                )
                if not ready:
                    self._offer_at = None
                    if self._offered and action is None:
                        action = "withdraw"
                elif not self._offered:
                    if self._offer_at is None:
                        self._offer_at = now + settings.unlock_delay
                    elif now >= self._offer_at:
                        self._offer_at = None
                        if action is None:
                            action = "offer"

        if action == "lock":
            if lockstate.lock_workstation():
                with self._lock:
                    self._locked = True
                    self._pending_lock_at = None
                    self._offer_at = None
                    self._last_action = "手机已远离，自动锁屏"
                log.info("手机已远离，执行自动锁屏")
            else:
                self._set_error("自动锁屏失败")
        elif action == "offer":
            self._offer_unlock()
        elif action == "withdraw":
            self._withdraw_offer("手机已离开或自动操作已关闭")

    def _offer_unlock(self) -> None:
        password = secret.load_secret()
        if not password:
            self._set_error("尚未保存 Windows 密码，无法提供手机解锁选项")
            with self._lock:
                self._offer_at = time.monotonic() + OFFER_RETRY_SECONDS
            return
        username = credential.account_name()
        domain = credential.account_domain()
        try:
            ok, response = unlock.send_unlock(
                username,
                password,
                domain,
                retries=1,
                connect_timeout=OFFER_CONNECT_TIMEOUT,
            )
        except unlock.UnlockError as exc:
            with self._lock:
                self._offer_at = time.monotonic() + OFFER_RETRY_SECONDS
                self._offer_failures += 1
                failures = self._offer_failures
                if failures == 1:
                    self._last_action = "正在等待锁屏界面加载…"
            if failures == 1 or failures % 30 == 0:
                log.info("提供锁屏解锁选项失败，稍后重试：%s", exc)
            return
        if not ok:
            with self._lock:
                self._offer_at = time.monotonic() + OFFER_RETRY_SECONDS
                self._offer_failures += 1
                failures = self._offer_failures
            if failures == 1 or failures % 30 == 0:
                log.warning("解锁组件拒绝解锁选项：%s", response)
            return

        with self._lock:
            self._offered = True
            self._offer_at = None
            self._offer_failures = 0
            still_ready = (
                not self._stop.is_set()
                and self.settings.click_unlock
                and not self.settings.paused
                and not self._remote
                and self._locked
                and self._presence_state == STATE_NEAR
            )
            if still_ready:
                self._last_action = "手机在附近 · 请在锁屏界面点击解锁"
                self._last_error = ""
        if not still_ready:
            self._withdraw_offer("状态已变化")
            return
        log.info("已向锁屏界面提供解锁选项，等待用户点击确认")

    def _withdraw_offer(self, reason: str) -> None:
        with self._lock:
            was_offered = self._offered
            self._offered = False
            self._offer_at = None
            self._offer_failures = 0
        if not was_offered:
            return
        try:
            ok, response = unlock.clear_pending()
        except unlock.UnlockError as exc:
            log.info("撤销锁屏解锁选项失败（锁屏界面可能已关闭）：%s", exc)
            return
        if ok:
            log.info("已撤销锁屏解锁选项（%s）", reason)
        else:
            log.warning("解锁组件撤销失败：%s", response)

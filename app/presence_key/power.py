from __future__ import annotations

import ctypes
import logging
import threading
from ctypes import wintypes

log = logging.getLogger(__name__)

PBT_APMRESUMEAUTOMATIC = 0x0012
PBT_APMRESUMESUSPEND = 0x0007
DEVICE_NOTIFY_CALLBACK = 2
ERROR_SUCCESS = 0

_powrprof = ctypes.WinDLL("powrprof", use_last_error=True)
_powrprof.PowerRegisterSuspendResumeNotification.argtypes = [
    wintypes.DWORD,
    ctypes.c_void_p,
    ctypes.POINTER(wintypes.HANDLE),
]
_powrprof.PowerRegisterSuspendResumeNotification.restype = wintypes.DWORD
_powrprof.PowerUnregisterSuspendResumeNotification.argtypes = [wintypes.HANDLE]
_powrprof.PowerUnregisterSuspendResumeNotification.restype = wintypes.DWORD

_CALLBACK = ctypes.WINFUNCTYPE(
    wintypes.ULONG, ctypes.c_void_p, wintypes.ULONG, ctypes.c_void_p
)


class _SubscribeParameters(ctypes.Structure):
    _fields_ = [("Callback", ctypes.c_void_p), ("Context", ctypes.c_void_p)]


class ResumeWatcher:
    def __init__(self, on_resume):
        self._on_resume = on_resume
        self._handle = None
        self._callback = None
        self._params = None

    def start(self) -> None:
        if self._handle:
            return

        def callback(context, event_type, setting):
            if event_type in (PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND):
                threading.Thread(
                    target=self._on_resume, name="resume-handler", daemon=True
                ).start()
            return ERROR_SUCCESS

        self._callback = _CALLBACK(callback)
        self._params = _SubscribeParameters(
            ctypes.cast(self._callback, ctypes.c_void_p), None
        )
        handle = wintypes.HANDLE()
        result = _powrprof.PowerRegisterSuspendResumeNotification(
            DEVICE_NOTIFY_CALLBACK, ctypes.byref(self._params), ctypes.byref(handle)
        )
        if result != ERROR_SUCCESS:
            log.warning("注册系统唤醒通知失败（错误 %s）", result)
            self._handle = None
            return
        self._handle = handle
        log.info("系统唤醒通知已注册")

    def stop(self) -> None:
        if not self._handle:
            return
        _powrprof.PowerUnregisterSuspendResumeNotification(self._handle)
        self._handle = None

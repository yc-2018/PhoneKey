from __future__ import annotations

import ctypes
import logging
import threading
from ctypes import wintypes

log = logging.getLogger(__name__)

PIPE_NAME = r"\\.\pipe\PhoneKeyVerify"
PIPE_ACCESS_DUPLEX = 0x00000003
FILE_FLAG_FIRST_PIPE_INSTANCE = 0x00080000
PIPE_TYPE_MESSAGE = 0x00000004
PIPE_READMODE_MESSAGE = 0x00000002
PIPE_WAIT = 0x00000000
PIPE_REJECT_REMOTE_CLIENTS = 0x00000008
INVALID_HANDLE_VALUE = wintypes.HANDLE(-1).value
ERROR_PIPE_CONNECTED = 535
ERROR_PIPE_BUSY = 231
ERROR_ACCESS_DENIED = 5
ERROR_ALREADY_EXISTS = 183

_SDDL = (
    "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;AU)"
    "(D;;GA;;;AN)(D;;GA;;;NU)"
)

_kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
_advapi32 = ctypes.WinDLL("advapi32", use_last_error=True)


class _SecurityAttributes(ctypes.Structure):
    _fields_ = [
        ("nLength", wintypes.DWORD),
        ("lpSecurityDescriptor", ctypes.c_void_p),
        ("bInheritHandle", wintypes.BOOL),
    ]


_kernel32.CreateNamedPipeW.argtypes = [
    wintypes.LPCWSTR,
    wintypes.DWORD,
    wintypes.DWORD,
    wintypes.DWORD,
    wintypes.DWORD,
    wintypes.DWORD,
    wintypes.DWORD,
    ctypes.c_void_p,
]
_kernel32.CreateNamedPipeW.restype = wintypes.HANDLE
_kernel32.ConnectNamedPipe.argtypes = [wintypes.HANDLE, ctypes.c_void_p]
_kernel32.ConnectNamedPipe.restype = wintypes.BOOL
_kernel32.DisconnectNamedPipe.argtypes = [wintypes.HANDLE]
_kernel32.DisconnectNamedPipe.restype = wintypes.BOOL
_kernel32.ReadFile.argtypes = [
    wintypes.HANDLE,
    ctypes.c_void_p,
    wintypes.DWORD,
    ctypes.POINTER(wintypes.DWORD),
    ctypes.c_void_p,
]
_kernel32.ReadFile.restype = wintypes.BOOL
_kernel32.WriteFile.argtypes = [
    wintypes.HANDLE,
    ctypes.c_void_p,
    wintypes.DWORD,
    ctypes.POINTER(wintypes.DWORD),
    ctypes.c_void_p,
]
_kernel32.WriteFile.restype = wintypes.BOOL
_kernel32.FlushFileBuffers.argtypes = [wintypes.HANDLE]
_kernel32.FlushFileBuffers.restype = wintypes.BOOL
_kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
_kernel32.CloseHandle.restype = wintypes.BOOL
_kernel32.LocalFree.argtypes = [ctypes.c_void_p]
_kernel32.LocalFree.restype = ctypes.c_void_p
_advapi32.ConvertStringSecurityDescriptorToSecurityDescriptorW.argtypes = [
    wintypes.LPCWSTR,
    wintypes.DWORD,
    ctypes.POINTER(ctypes.c_void_p),
    ctypes.POINTER(wintypes.DWORD),
]
_advapi32.ConvertStringSecurityDescriptorToSecurityDescriptorW.restype = wintypes.BOOL


class PresenceVerifyServer:
    def __init__(self, presence):
        self._presence = presence
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        self._stop.clear()
        self._thread = threading.Thread(
            target=self._run, name="presence-verify", daemon=True
        )
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()

    def _run(self) -> None:
        descriptor = ctypes.c_void_p()
        if not _advapi32.ConvertStringSecurityDescriptorToSecurityDescriptorW(
            _SDDL, 1, ctypes.byref(descriptor), None
        ):
            log.error(
                "创建手机校验管道安全描述符失败（Windows 错误 %s）",
                ctypes.get_last_error(),
            )
            return
        attributes = _SecurityAttributes(
            ctypes.sizeof(_SecurityAttributes), descriptor, False
        )
        handle = INVALID_HANDLE_VALUE
        while not self._stop.is_set():
            handle = _kernel32.CreateNamedPipeW(
                PIPE_NAME,
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE
                | PIPE_READMODE_MESSAGE
                | PIPE_WAIT
                | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                64,
                64,
                0,
                ctypes.byref(attributes),
            )
            if handle != INVALID_HANDLE_VALUE:
                break
            error = ctypes.get_last_error()
            if error not in (
                ERROR_ACCESS_DENIED,
                ERROR_PIPE_BUSY,
                ERROR_ALREADY_EXISTS,
            ):
                log.warning("创建手机校验管道失败（Windows 错误 %s）", error)
            self._stop.wait(1.0)
        if handle == INVALID_HANDLE_VALUE:
            if descriptor:
                _kernel32.LocalFree(descriptor)
            return
        log.info("手机校验管道已启动")
        try:
            while not self._stop.is_set():
                if not _kernel32.ConnectNamedPipe(handle, None):
                    error = ctypes.get_last_error()
                    if error != ERROR_PIPE_CONNECTED:
                        if self._stop.is_set():
                            break
                        log.warning(
                            "连接手机校验管道失败（Windows 错误 %s）", error
                        )
                        self._stop.wait(0.5)
                        continue
                if self._stop.is_set():
                    break
                buffer = ctypes.create_string_buffer(64)
                read = wintypes.DWORD()
                if _kernel32.ReadFile(
                    handle,
                    buffer,
                    len(buffer) - 1,
                    ctypes.byref(read),
                    None,
                ):
                    command = buffer.raw[: read.value].decode(
                        "ascii", "ignore"
                    )
                    if command.startswith("VERIFY"):
                        near = self._presence.verify_near()
                        response = b"OK" if near else b"DENY"
                        log.info(
                            "锁屏点击校验：%s",
                            "手机在附近" if near else "手机不在附近",
                        )
                    else:
                        response = b"ERR"
                    written = wintypes.DWORD()
                    _kernel32.WriteFile(
                        handle,
                        response,
                        len(response),
                        ctypes.byref(written),
                        None,
                    )
                    _kernel32.FlushFileBuffers(handle)
                _kernel32.DisconnectNamedPipe(handle)
        finally:
            _kernel32.CloseHandle(handle)
            if descriptor:
                _kernel32.LocalFree(descriptor)
            log.info("手机校验管道已停止")

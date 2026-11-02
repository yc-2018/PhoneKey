from __future__ import annotations

import ctypes
import logging
import os
import time
import winreg
from ctypes import wintypes
from pathlib import Path

from .secret import PASSWORD_MAX

log = logging.getLogger(__name__)

PIPE_NAME = r"\\.\pipe\CredentialProviderPipe"
CP_GUID = "{E0A8C5B2-9F3D-4E7A-B1C6-8D2F5A3E9B70}"
CP_KEY = (
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication"
    rf"\Credential Providers\{CP_GUID}"
)

GENERIC_READ = 0x80000000
GENERIC_WRITE = 0x40000000
OPEN_EXISTING = 3
INVALID_HANDLE_VALUE = wintypes.HANDLE(-1).value
PIPE_READMODE_MESSAGE = 0x00000002
ERROR_PIPE_BUSY = 231
ERROR_FILE_NOT_FOUND = 2

_kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
_kernel32.CreateFileW.argtypes = [
    wintypes.LPCWSTR,
    wintypes.DWORD,
    wintypes.DWORD,
    ctypes.c_void_p,
    wintypes.DWORD,
    wintypes.DWORD,
    wintypes.HANDLE,
]
_kernel32.CreateFileW.restype = wintypes.HANDLE
_kernel32.WaitNamedPipeW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD]
_kernel32.WaitNamedPipeW.restype = wintypes.BOOL
_kernel32.SetNamedPipeHandleState.argtypes = [
    wintypes.HANDLE,
    ctypes.POINTER(wintypes.DWORD),
    ctypes.c_void_p,
    ctypes.c_void_p,
]
_kernel32.SetNamedPipeHandleState.restype = wintypes.BOOL
_kernel32.WriteFile.argtypes = [
    wintypes.HANDLE,
    ctypes.c_void_p,
    wintypes.DWORD,
    ctypes.POINTER(wintypes.DWORD),
    ctypes.c_void_p,
]
_kernel32.WriteFile.restype = wintypes.BOOL
_kernel32.ReadFile.argtypes = [
    wintypes.HANDLE,
    ctypes.c_void_p,
    wintypes.DWORD,
    ctypes.POINTER(wintypes.DWORD),
    ctypes.c_void_p,
]
_kernel32.ReadFile.restype = wintypes.BOOL
_kernel32.PeekNamedPipe.argtypes = [
    wintypes.HANDLE,
    ctypes.c_void_p,
    wintypes.DWORD,
    ctypes.POINTER(wintypes.DWORD),
    ctypes.POINTER(wintypes.DWORD),
    ctypes.POINTER(wintypes.DWORD),
]
_kernel32.PeekNamedPipe.restype = wintypes.BOOL
_kernel32.FlushFileBuffers.argtypes = [wintypes.HANDLE]
_kernel32.FlushFileBuffers.restype = wintypes.BOOL
_kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
_kernel32.CloseHandle.restype = wintypes.BOOL


class UnlockError(RuntimeError):
    pass


def cp_installed() -> bool:
    system_root = os.environ.get("SystemRoot", r"C:\Windows")
    if not (Path(system_root) / "System32" / "UnlockProvider.dll").exists():
        return False
    try:
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, CP_KEY):
            return True
    except OSError:
        return False


def _connect(timeout: float = 2.0) -> int:
    deadline = time.monotonic() + timeout
    while True:
        handle = _kernel32.CreateFileW(
            PIPE_NAME,
            GENERIC_READ | GENERIC_WRITE,
            0,
            None,
            OPEN_EXISTING,
            0,
            None,
        )
        if handle and handle != INVALID_HANDLE_VALUE:
            return handle
        error = ctypes.get_last_error()
        if error == ERROR_PIPE_BUSY and time.monotonic() < deadline:
            _kernel32.WaitNamedPipeW(PIPE_NAME, 100)
            continue
        if error == ERROR_FILE_NOT_FOUND:
            raise UnlockError("解锁组件尚未加载（锁屏界面未出现）")
        raise UnlockError(f"无法连接解锁组件（Windows 错误 {error}）")


def _set_message_mode(handle: int) -> None:
    mode = wintypes.DWORD(PIPE_READMODE_MESSAGE)
    if not _kernel32.SetNamedPipeHandleState(
        handle, ctypes.byref(mode), None, None
    ):
        raise UnlockError(
            f"设置管道模式失败（Windows 错误 {ctypes.get_last_error()}）"
        )


def _send_once(payload: bytes, connect_timeout: float = 2.0) -> str:
    handle = _connect(timeout=connect_timeout)
    try:
        _set_message_mode(handle)
        written = wintypes.DWORD()
        if not _kernel32.WriteFile(
            handle, payload, len(payload), ctypes.byref(written), None
        ):
            raise UnlockError(f"发送解锁命令失败（Windows 错误 {ctypes.get_last_error()}）")
        _kernel32.FlushFileBuffers(handle)

        deadline = time.monotonic() + 3.0
        available = wintypes.DWORD()
        while True:
            if not _kernel32.PeekNamedPipe(
                handle, None, 0, None, ctypes.byref(available), None
            ):
                raise UnlockError(f"读取解锁结果失败（Windows 错误 {ctypes.get_last_error()}）")
            if available.value:
                break
            if time.monotonic() > deadline:
                raise UnlockError("等待解锁组件响应超时")
            time.sleep(0.05)

        buffer = ctypes.create_string_buffer(256)
        read = wintypes.DWORD()
        if not _kernel32.ReadFile(
            handle, buffer, len(buffer) - 1, ctypes.byref(read), None
        ):
            raise UnlockError(f"读取解锁结果失败（Windows 错误 {ctypes.get_last_error()}）")
        return buffer.raw[: read.value].decode("utf-8", "replace")
    finally:
        _kernel32.CloseHandle(handle)


def send_unlock(
    username: str,
    password: str,
    domain: str = ".",
    retries: int = 8,
    delay: float = 0.5,
    connect_timeout: float = 2.0,
) -> tuple[bool, str]:
    if not username:
        raise UnlockError("用户名为空")
    if not password:
        raise UnlockError("尚未保存密码")
    if len(password) > PASSWORD_MAX:
        raise UnlockError("密码过长")
    payload = f"UNLOCK:{domain}\\{username}:{password}".encode("utf-8")
    if len(payload) >= 500:
        raise UnlockError("凭据长度超出解锁组件限制")

    last_error: UnlockError | None = None
    for attempt in range(retries):
        try:
            response = _send_once(payload, connect_timeout=connect_timeout)
            return response.strip() == "OK", response
        except UnlockError as exc:
            last_error = exc
            if attempt < retries - 1:
                time.sleep(delay)
    raise last_error or UnlockError("解锁失败")


def clear_pending(connect_timeout: float = 0.6) -> tuple[bool, str]:
    """Withdraw the unlock offer held by the credential provider."""
    response = _send_once(b"CLEAR", connect_timeout=connect_timeout)
    return response.strip() == "OK", response

from __future__ import annotations

import ctypes
import os
from ctypes import wintypes

_advapi32 = ctypes.WinDLL("advapi32", use_last_error=True)
_kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

_advapi32.GetUserNameW.argtypes = [wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
_advapi32.GetUserNameW.restype = wintypes.BOOL
_advapi32.LogonUserW.argtypes = [
    wintypes.LPCWSTR,
    wintypes.LPCWSTR,
    wintypes.LPCWSTR,
    wintypes.DWORD,
    wintypes.DWORD,
    ctypes.POINTER(wintypes.HANDLE),
]
_advapi32.LogonUserW.restype = wintypes.BOOL
_kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
_kernel32.CloseHandle.restype = wintypes.BOOL

LOGON32_LOGON_NETWORK = 3
LOGON32_PROVIDER_DEFAULT = 0

ERROR_TEXT = {
    1326: "用户名或密码不正确",
    1327: "该账户不允许使用空密码",
    1330: "密码已过期",
    1331: "账户已被禁用",
    1385: "该账户没有远程登录权限",
    1907: "首次登录前必须先修改密码",
}


def account_name() -> str:
    size = wintypes.DWORD(256)
    buffer = ctypes.create_unicode_buffer(size.value)
    if not _advapi32.GetUserNameW(buffer, ctypes.byref(size)):
        return os.environ.get("USERNAME", "")
    return buffer.value


def account_domain() -> str:
    return os.environ.get("USERDOMAIN", ".") or "."


def verify_password(username: str, password: str, domain: str | None = None) -> tuple[bool, str]:
    token = wintypes.HANDLE()
    domain = domain or account_domain()
    ok = _advapi32.LogonUserW(
        username,
        domain,
        password,
        LOGON32_LOGON_NETWORK,
        LOGON32_PROVIDER_DEFAULT,
        ctypes.byref(token),
    )
    if ok:
        if token:
            _kernel32.CloseHandle(token)
        return True, ""
    error = ctypes.get_last_error()
    return False, ERROR_TEXT.get(error, f"Windows 错误 {error}")

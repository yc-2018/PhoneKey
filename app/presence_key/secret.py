from __future__ import annotations

import ctypes
import logging
from ctypes import wintypes
from pathlib import Path

from .paths import secret_path

log = logging.getLogger(__name__)

CRYPTPROTECT_UI_FORBIDDEN = 0x01
SECRET_MAGIC = b"MPK1"
PASSWORD_MAX = 200


class _DataBlob(ctypes.Structure):
    _fields_ = [
        ("cbData", wintypes.DWORD),
        ("pbData", ctypes.POINTER(ctypes.c_char)),
    ]


_crypt32 = ctypes.WinDLL("crypt32", use_last_error=True)
_kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

_crypt32.CryptProtectData.argtypes = [
    ctypes.POINTER(_DataBlob),
    wintypes.LPCWSTR,
    ctypes.POINTER(_DataBlob),
    ctypes.c_void_p,
    ctypes.c_void_p,
    wintypes.DWORD,
    ctypes.POINTER(_DataBlob),
]
_crypt32.CryptProtectData.restype = wintypes.BOOL
_crypt32.CryptUnprotectData.argtypes = [
    ctypes.POINTER(_DataBlob),
    ctypes.POINTER(wintypes.LPWSTR),
    ctypes.POINTER(_DataBlob),
    ctypes.c_void_p,
    ctypes.c_void_p,
    wintypes.DWORD,
    ctypes.POINTER(_DataBlob),
]
_crypt32.CryptUnprotectData.restype = wintypes.BOOL
_kernel32.LocalFree.argtypes = [wintypes.HLOCAL]
_kernel32.LocalFree.restype = wintypes.HLOCAL


def _make_blob(data: bytes) -> tuple[_DataBlob, ctypes.Array]:
    buffer = ctypes.create_string_buffer(data, len(data))
    blob = _DataBlob(len(data), ctypes.cast(buffer, ctypes.POINTER(ctypes.c_char)))
    return blob, buffer


def _protect(data: bytes) -> bytes:
    in_blob, _buffer = _make_blob(data)
    out_blob = _DataBlob()
    if not _crypt32.CryptProtectData(
        ctypes.byref(in_blob),
        "PhoneKey",
        None,
        None,
        None,
        CRYPTPROTECT_UI_FORBIDDEN,
        ctypes.byref(out_blob),
    ):
        raise OSError(ctypes.get_last_error(), "CryptProtectData 失败")
    try:
        return ctypes.string_at(out_blob.pbData, out_blob.cbData)
    finally:
        _kernel32.LocalFree(out_blob.pbData)


def _unprotect(data: bytes) -> bytes:
    in_blob, _buffer = _make_blob(data)
    out_blob = _DataBlob()
    if not _crypt32.CryptUnprotectData(
        ctypes.byref(in_blob),
        None,
        None,
        None,
        None,
        CRYPTPROTECT_UI_FORBIDDEN,
        ctypes.byref(out_blob),
    ):
        raise OSError(ctypes.get_last_error(), "CryptUnprotectData 失败")
    try:
        return ctypes.string_at(out_blob.pbData, out_blob.cbData)
    finally:
        _kernel32.LocalFree(out_blob.pbData)


def save_secret(password: str) -> None:
    if not password:
        raise ValueError("密码不能为空")
    if len(password) > PASSWORD_MAX:
        raise ValueError("密码过长")
    payload = SECRET_MAGIC + _protect(password.encode("utf-8"))
    path = secret_path()
    tmp = path.with_suffix(".bin.tmp")
    tmp.write_bytes(payload)
    tmp.replace(path)


def load_secret() -> str | None:
    path = secret_path()
    if not path.exists():
        return None
    try:
        payload = path.read_bytes()
        if not payload.startswith(SECRET_MAGIC):
            return None
        return _unprotect(payload[len(SECRET_MAGIC):]).decode("utf-8")
    except (OSError, UnicodeDecodeError) as exc:
        log.error("读取保存的密码失败: %s", exc)
        return None


def has_secret() -> bool:
    path = secret_path()
    try:
        with path.open("rb") as handle:
            return handle.read(len(SECRET_MAGIC)) == SECRET_MAGIC
    except OSError:
        return False


def clear_secret() -> None:
    try:
        secret_path().unlink(missing_ok=True)
    except OSError as exc:
        log.warning("删除保存的密码失败: %s", exc)


def stored_password_hint() -> str:
    return "已保存" if has_secret() else "未保存"

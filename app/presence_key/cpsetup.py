from __future__ import annotations

import logging
import subprocess
from pathlib import Path

from .paths import installer_dir

log = logging.getLogger(__name__)


def _run_script(script: Path) -> tuple[bool, str]:
    if not script.exists():
        return False, f"找不到脚本：{script}"
    quoted = str(script).replace("'", "''")
    command = (
        "$p = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru "
        f"-ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File','{quoted}'); "
        "exit $p.ExitCode"
    )
    try:
        result = subprocess.run(
            ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", command],
            capture_output=True,
            text=True,
            timeout=120,
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        log.error("运行安装脚本失败: %s", exc)
        return False, str(exc)
    if result.returncode == 0:
        return True, result.stdout.strip()
    message = (result.stderr or result.stdout).strip()
    log.error("安装脚本返回 %s: %s", result.returncode, message)
    return False, message or f"脚本返回 {result.returncode}"


def install_cp() -> tuple[bool, str]:
    return _run_script(installer_dir() / "install_cp.ps1")


def uninstall_cp() -> tuple[bool, str]:
    return _run_script(installer_dir() / "uninstall_cp.ps1")

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use log::{error, info};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn script_candidates(resource_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = resource_dir {
        candidates.push(dir.join("_up_").join("installer"));
        candidates.push(dir.join("installer"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("installer"));
            candidates.push(parent.join("_up_").join("installer"));
        }
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("installer"),
    );
    candidates
}

fn find_script(name: &str, resource_dir: Option<&Path>) -> Option<PathBuf> {
    script_candidates(resource_dir)
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

// PowerShell 5.1 的 -File 不接受 \\?\ 长路径前缀。
fn plain_path(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn run_script(script: &Path) -> Result<String, String> {
    let script = plain_path(script);
    info!("运行安装脚本: {}", script.display());
    let quoted = script.display().to_string().replace('\'', "''");
    let command = format!(
        "$p = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru \
         -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File','{quoted}'); \
         exit $p.ExitCode"
    );
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &command,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|err| format!("启动安装脚本失败：{err}"))?;

    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    info!("安装脚本执行成功: {}", script.display());
                    return Ok(String::new());
                }
                let message = format!("安装脚本返回 {}", status.code().unwrap_or(-1));
                error!("{message}");
                return Err(message);
            }
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    return Err("安装脚本等待超时".to_string());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(err) => return Err(format!("等待安装脚本失败：{err}")),
        }
    }
}

pub fn install_cp(resource_dir: Option<&Path>) -> Result<(), String> {
    let script = find_script("install_cp.ps1", resource_dir)
        .ok_or_else(|| "找不到安装脚本 install_cp.ps1".to_string())?;
    run_script(&script).map(|_| ())
}

pub fn uninstall_cp(resource_dir: Option<&Path>) -> Result<(), String> {
    let script = find_script("uninstall_cp.ps1", resource_dir)
        .ok_or_else(|| "找不到卸载脚本 uninstall_cp.ps1".to_string())?;
    run_script(&script).map(|_| ())
}

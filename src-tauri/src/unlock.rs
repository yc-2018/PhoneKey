use std::path::PathBuf;
use std::time::{Duration, Instant};

use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;
use windows::core::{Error, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE,
    OPEN_EXISTING,
};
use windows::Win32::System::Pipes::{
    PeekNamedPipe, SetNamedPipeHandleState, WaitNamedPipeW, PIPE_READMODE_MESSAGE,
};

use crate::secret::PASSWORD_MAX;
use crate::sys::win32_code;

pub const PIPE_NAME: &str = r"\\.\pipe\CredentialProviderPipe";
pub const CP_GUID: &str = "{E0A8C5B2-9F3D-4E7A-B1C6-8D2F5A3E9B70}";
const CP_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers";

#[derive(Debug)]
pub struct UnlockError(pub String);

impl std::fmt::Display for UnlockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UnlockError {}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn cp_installed() -> bool {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    let dll = PathBuf::from(system_root)
        .join("System32")
        .join("UnlockProvider.dll");
    if !dll.is_file() {
        return false;
    }
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(format!(r"{CP_KEY}\{CP_GUID}"))
        .is_ok()
}

fn pipe_code(error: &Error) -> u32 {
    win32_code(error)
}

fn connect(timeout: Duration) -> Result<HANDLE, UnlockError> {
    let name = wide(PIPE_NAME);
    let deadline = Instant::now() + timeout;
    loop {
        let result = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        };
        match result {
            Ok(handle) => return Ok(handle),
            Err(error) => {
                let code = pipe_code(&error);
                if code == 231 && Instant::now() < deadline {
                    unsafe {
                        let _ = WaitNamedPipeW(PCWSTR(name.as_ptr()), 100);
                    }
                    continue;
                }
                if code == 2 {
                    return Err(UnlockError("解锁组件尚未加载（锁屏界面未出现）".to_string()));
                }
                return Err(UnlockError(format!("无法连接解锁组件（Windows 错误 {code}）")));
            }
        }
    }
}

fn send_once(payload: &[u8], connect_timeout: Duration) -> Result<String, UnlockError> {
    let handle = connect(connect_timeout)?;
    let result = (|| {
        unsafe {
            SetNamedPipeHandleState(
                handle,
                Some(&PIPE_READMODE_MESSAGE),
                None,
                None,
            )
            .map_err(|error| {
                UnlockError(format!(
                    "设置管道模式失败（Windows 错误 {}）",
                    pipe_code(&error)
                ))
            })?;
            let mut written = 0u32;
            WriteFile(handle, Some(payload), Some(&mut written), None).map_err(|error| {
                UnlockError(format!(
                    "发送解锁命令失败（Windows 错误 {}）",
                    pipe_code(&error)
                ))
            })?;
            let _ = FlushFileBuffers(handle);
        }

        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let mut available = 0u32;
            unsafe {
                PeekNamedPipe(handle, None, 0, None, Some(&mut available), None).map_err(
                    |error| {
                        UnlockError(format!(
                            "读取解锁结果失败（Windows 错误 {}）",
                            pipe_code(&error)
                        ))
                    },
                )?;
            }
            if available > 0 {
                break;
            }
            if Instant::now() > deadline {
                return Err(UnlockError("等待解锁组件响应超时".to_string()));
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        let mut buffer = [0u8; 256];
        let mut read = 0u32;
        unsafe {
            ReadFile(handle, Some(&mut buffer), Some(&mut read), None).map_err(|error| {
                UnlockError(format!(
                    "读取解锁结果失败（Windows 错误 {}）",
                    pipe_code(&error)
                ))
            })?;
        }
        Ok(String::from_utf8_lossy(&buffer[..read as usize]).to_string())
    })();
    unsafe {
        let _ = CloseHandle(handle);
    }
    result
}

pub fn send_unlock(
    username: &str,
    password: &str,
    domain: &str,
    retries: usize,
    connect_timeout: Duration,
) -> Result<(bool, String), UnlockError> {
    if username.is_empty() {
        return Err(UnlockError("用户名为空".to_string()));
    }
    if password.is_empty() {
        return Err(UnlockError("尚未保存密码".to_string()));
    }
    if password.chars().count() > PASSWORD_MAX {
        return Err(UnlockError("密码过长".to_string()));
    }
    let payload = format!("UNLOCK:{domain}\\{username}:{password}");
    if payload.len() >= 500 {
        return Err(UnlockError("凭据长度超出解锁组件限制".to_string()));
    }

    let mut last_error = None;
    for attempt in 0..retries.max(1) {
        match send_once(payload.as_bytes(), connect_timeout) {
            Ok(response) => return Ok((response.trim() == "OK", response)),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < retries {
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| UnlockError("解锁失败".to_string())))
}

pub fn clear_pending(connect_timeout: Duration) -> Result<(bool, String), UnlockError> {
    let response = send_once(b"CLEAR", connect_timeout)?;
    Ok((response.trim() == "OK", response))
}

pub fn query_state(connect_timeout: Duration) -> Result<String, UnlockError> {
    send_once(b"QUERY", connect_timeout)
}

pub fn send_presence_status(
    near: bool,
    connect_timeout: Duration,
) -> Result<(bool, String), UnlockError> {
    let payload: &[u8] = if near { b"STATUS:NEAR" } else { b"STATUS:AWAY" };
    let response = send_once(payload, connect_timeout)?;
    Ok((response.trim() == "OK", response))
}

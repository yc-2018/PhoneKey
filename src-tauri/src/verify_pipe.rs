use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use log::{info, warn};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, GENERIC_READ, GENERIC_WRITE, HLOCAL,
    INVALID_HANDLE_VALUE,
};
use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE,
    FILE_SHARE_MODE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_MESSAGE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_WAIT,
};

use crate::config::Settings;
use crate::presence::{PresenceMonitor, VERIFY_TIMEOUT};
use crate::secret;
use crate::sys::{self, win32_code};
use crate::wifi;

const PIPE_NAME: &str = r"\\.\pipe\PhoneKeyVerify";
const SDDL: &str = "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;AU)(D;;GA;;;AN)(D;;GA;;;NU)";
const CRED_PIPE_NAME: &str = r"\\.\pipe\PhoneKeyCredentials";
const CRED_SDDL: &str =
    "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;OW)(D;;GA;;;AN)(D;;GA;;;NU)";
const CRED_RESPONSE_MAX: usize = 4000;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct VerifyServer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl VerifyServer {
    pub fn start(presence: Arc<PresenceMonitor>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("presence-verify".to_string())
            .spawn(move || run(presence, thread_stop))
            .ok();
        Self { stop, thread }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        wake_pipe(PIPE_NAME);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for VerifyServer {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct CredentialServer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CredentialServer {
    pub fn start(presence: Arc<PresenceMonitor>, settings: Arc<RwLock<Settings>>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("credential-pipe".to_string())
            .spawn(move || run_credentials(presence, settings, thread_stop))
            .ok();
        Self { stop, thread }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        wake_pipe(CRED_PIPE_NAME);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for CredentialServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn wake_pipe(name: &str) {
    let name = wide(name);
    if let Ok(handle) = unsafe {
        CreateFileW(
            PCWSTR(name.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_MODE(0),
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    } {
        unsafe {
            let _ = CloseHandle(handle);
        }
    }
}

fn run(presence: Arc<PresenceMonitor>, stop: Arc<AtomicBool>) {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    if let Err(error) = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide(SDDL).as_ptr()),
            1,
            &mut descriptor,
            None,
        )
    } {
        warn!("创建手机校验管道安全描述符失败: {error}");
        return;
    }

    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let name = wide(PIPE_NAME);

    let mut handle = INVALID_HANDLE_VALUE;
    while !stop.load(Ordering::SeqCst) {
        handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,

                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                64,
                64,
                0,
                Some(&attributes),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            break;
        }
        let code = unsafe { GetLastError() };
        if !matches!(code.0, 5 | 183 | 231) {
            warn!("创建手机校验管道失败（Windows 错误 {}）", code.0);
        }
        thread::sleep(Duration::from_secs(1));
    }

    if handle == INVALID_HANDLE_VALUE {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
        return;
    }

    info!("手机校验管道已启动");
    while !stop.load(Ordering::SeqCst) {
        if let Err(error) = unsafe { ConnectNamedPipe(handle, None) } {
            if win32_code(&error) != 535 {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(Duration::from_millis(500));
                continue;
            }
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }

        let mut buffer = [0u8; 64];
        let mut read = 0u32;
        if unsafe { ReadFile(handle, Some(&mut buffer), Some(&mut read), None) }.is_ok() {
            let command = String::from_utf8_lossy(&buffer[..read as usize]);
            let response: &[u8] = if command.starts_with("VERIFY") {
                info!("收到锁屏点击校验请求");
                wifi::probe_now(&presence);
                let near = presence.verify_near(VERIFY_TIMEOUT);
                info!(
                    "锁屏点击校验：{}",
                    if near { "手机在附近" } else { "手机不在附近" }
                );
                if near { b"OK" } else { b"DENY" }
            } else {
                b"ERR"
            };
            let mut written = 0u32;
            let _ = unsafe { WriteFile(handle, Some(response), Some(&mut written), None) };
            let _ = unsafe { FlushFileBuffers(handle) };
        }
        let _ = unsafe { DisconnectNamedPipe(handle) };
    }

    unsafe {
        let _ = CloseHandle(handle);
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }
    info!("手机校验管道已停止");
}

fn run_credentials(
    presence: Arc<PresenceMonitor>,
    settings: Arc<RwLock<Settings>>,
    stop: Arc<AtomicBool>,
) {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    if let Err(error) = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide(CRED_SDDL).as_ptr()),
            1,
            &mut descriptor,
            None,
        )
    } {
        warn!("创建手机解锁管道安全描述符失败: {error}");
        return;
    }

    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let name = wide(CRED_PIPE_NAME);

    let mut handle = INVALID_HANDLE_VALUE;
    while !stop.load(Ordering::SeqCst) {
        handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                CRED_RESPONSE_MAX as u32,
                64,
                0,
                Some(&attributes),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            break;
        }
        let code = unsafe { GetLastError() };
        if !matches!(code.0, 5 | 183 | 231) {
            warn!("创建手机解锁管道失败（Windows 错误 {}）", code.0);
        }
        thread::sleep(Duration::from_secs(1));
    }

    if handle == INVALID_HANDLE_VALUE {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
        return;
    }

    info!("手机解锁管道已就绪");
    while !stop.load(Ordering::SeqCst) {
        if let Err(error) = unsafe { ConnectNamedPipe(handle, None) } {
            if win32_code(&error) != 535 {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(Duration::from_millis(500));
                continue;
            }
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }

        let mut buffer = [0u8; 32];
        let mut read = 0u32;
        if unsafe { ReadFile(handle, Some(&mut buffer), Some(&mut read), None) }.is_ok() {
            let command = String::from_utf8_lossy(&buffer[..read as usize]);
            let mut response = if command.starts_with("PING") {
                b"READY".to_vec()
            } else if command.starts_with("GET") {
                credential_response(&presence, &settings)
            } else {
                b"ERR".to_vec()
            };
            let mut written = 0u32;
            let _ = unsafe { WriteFile(handle, Some(response.as_slice()), Some(&mut written), None) };
            let _ = unsafe { FlushFileBuffers(handle) };
            unsafe {
                std::ptr::write_bytes(response.as_mut_ptr(), 0, response.len());
            }
        }
        let _ = unsafe { DisconnectNamedPipe(handle) };
    }

    unsafe {
        let _ = CloseHandle(handle);
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }
    info!("手机解锁管道已停止");
}

fn credential_response(presence: &PresenceMonitor, settings: &RwLock<Settings>) -> Vec<u8> {
    let (enabled, paused) = {
        let settings = settings
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        (settings.click_unlock, settings.paused)
    };
    if !enabled || paused || sys::is_remote_session() {
        info!("锁屏凭据请求：手机解锁已关闭");
        return b"OFF".to_vec();
    }
    if !secret::has_secret() {
        info!("锁屏凭据请求：尚未保存密码");
        return b"NOCRED".to_vec();
    }
    info!("收到锁屏凭据请求");
    wifi::probe_now(presence);
    if !presence.verify_near(VERIFY_TIMEOUT) {
        info!("锁屏凭据请求：手机不在附近，已拒绝");
        return b"DENY".to_vec();
    }
    let Some(mut password) = secret::load_secret() else {
        warn!("锁屏凭据请求：读取保存的密码失败");
        return b"NOCRED".to_vec();
    };
    let username = sys::account_name();
    let domain = sys::account_domain();
    let response = match build_credential_response(&domain, &username, &password) {
        Some(response) => {
            info!("锁屏凭据请求：手机在附近，已允许");
            response
        }
        None => {
            warn!("锁屏凭据请求：账户信息无效");
            b"NOCRED".to_vec()
        }
    };
    unsafe {
        std::ptr::write_bytes(password.as_mut_ptr(), 0, password.len());
    }
    response
}

fn build_credential_response(domain: &str, username: &str, password: &str) -> Option<Vec<u8>> {
    if username.is_empty()
        || password.is_empty()
        || domain.contains('\0')
        || username.contains('\0')
        || password.contains('\0')
    {
        return None;
    }
    let mut response = Vec::with_capacity(domain.len() + username.len() + password.len() + 8);
    response.extend_from_slice(b"OK");
    response.push(0);
    response.extend_from_slice(domain.as_bytes());
    response.push(0);
    response.extend_from_slice(username.as_bytes());
    response.push(0);
    response.extend_from_slice(password.as_bytes());
    response.push(0);
    if response.len() > CRED_RESPONSE_MAX {
        return None;
    }
    Some(response)
}

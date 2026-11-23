use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::win::*;

const PIPE_NAME: &str = r"\\.\pipe\CredentialProviderPipe";
const VERIFY_PIPE_NAME: &str = r"\\.\pipe\PhoneKeyVerify";
const CRED_PIPE_NAME: &str = r"\\.\pipe\PhoneKeyCredentials";
const PIPE_BUFFER_SIZE: usize = 512;
const PIPE_READ_TIMEOUT_MS: u32 = 5000;
const SDDL: &str =
    "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;AU)(D;;GA;;;AN)(D;;GA;;;NU)";

pub const WM_UNLOCK_CREDENTIAL: u32 = WM_APP + 1;
pub const VERIFY_TIMEOUT_MS: u32 = 3200;
pub const CREDENTIAL_TIMEOUT_MS: u32 = 4000;

pub enum CredentialFetch {
    Ok {
        domain: String,
        username: String,
        password: String,
    },
    Deny,
    Off,
    NoCred,
    Error,
}

pub struct CredentialData {
    pub domain: String,
    pub username: String,
    pub password: String,
    pub pending: bool,
    pub nearby: Option<bool>,
}

impl Default for CredentialData {
    fn default() -> Self {
        Self {
            domain: String::new(),
            username: String::new(),
            password: String::new(),
            pending: false,
            nearby: None,
        }
    }
}

impl CredentialData {
    pub fn clear_password(&mut self) {
        wipe(&mut self.password);
    }
}

pub struct Shared {
    data: Mutex<CredentialData>,
    hwnd: AtomicIsize,
    pending_change: AtomicBool,
    stop_event: AtomicIsize,
    listener: Mutex<Option<JoinHandle<()>>>,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            data: Mutex::new(CredentialData::default()),
            hwnd: AtomicIsize::new(0),
            pending_change: AtomicBool::new(false),
            stop_event: AtomicIsize::new(0),
            listener: Mutex::new(None),
        })
    }

    pub fn set_hwnd(&self, hwnd: HWND) {
        self.hwnd.store(hwnd, Ordering::Release);
    }

    pub fn take_pending_change(&self) -> bool {
        self.pending_change.swap(false, Ordering::AcqRel)
    }

    pub fn set_credentials(&self, domain: String, username: String, password: String) {
        let mut data = self.lock_data();
        data.domain = clip(domain);
        data.username = clip(username);
        wipe(&mut data.password);
        data.password = clip(password);
        data.pending = true;
        drop(data);
        self.notify();
    }

    pub fn clear_pending(&self) {
        self.clear_pending_inner();
        self.notify();
    }

    pub fn clear_pending_silent(&self) {
        self.clear_pending_inner();
    }

    fn clear_pending_inner(&self) {
        let mut data = self.lock_data();
        data.pending = false;
        data.clear_password();
    }

    pub fn set_nearby(&self, nearby: bool) {
        self.lock_data().nearby = Some(nearby);
        self.notify();
    }

    pub fn set_nearby_silent(&self, nearby: bool) {
        self.lock_data().nearby = Some(nearby);
    }

    pub fn set_username(&self, username: String) {
        self.lock_data().username = clip(username);
    }

    pub fn set_password(&self, password: String) {
        let mut data = self.lock_data();
        wipe(&mut data.password);
        data.password = clip(password);
    }

    pub fn mark_change_pending(&self) {
        self.pending_change.store(true, Ordering::Release);
    }

    pub fn pending(&self) -> bool {
        self.lock_data().pending
    }

    pub fn label(&self) -> String {
        "点击检测手机并解锁".to_string()
    }

    pub fn state_code(&self) -> &'static str {
        let data = self.lock_data();
        match (data.pending, data.nearby) {
            (false, _) => "WAIT",
            (true, None) => "READY",
            (true, Some(true)) => "NEAR",
            (true, Some(false)) => "AWAY",
        }
    }

    pub fn username(&self) -> String {
        self.lock_data().username.clone()
    }

    pub fn serialization_copy(&self) -> (bool, String, String, String) {
        let data = self.lock_data();
        (
            data.pending,
            data.domain.clone(),
            data.username.clone(),
            data.password.clone(),
        )
    }

    pub fn clear_password_only(&self) {
        self.lock_data().clear_password();
    }

    pub fn notify(&self) {
        let hwnd = self.hwnd.load(Ordering::Acquire);
        if hwnd != 0 {
            let posted = unsafe {
                PostMessageW(hwnd, WM_UNLOCK_CREDENTIAL, 0, 0)
            };
            if posted == 0 {
                self.pending_change.store(true, Ordering::Release);
            }
        } else {
            self.pending_change.store(true, Ordering::Release);
        }
    }

    fn lock_data(&self) -> std::sync::MutexGuard<'_, CredentialData> {
        self.data.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    pub fn start_listener(self: &Arc<Self>) {
        let mut listener = self
            .listener
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if listener.is_some() {
            dbg("CP: listener already running\n");
            return;
        }
        let stop_event = unsafe {
            CreateEventW(ptr::null(), 1, 0, ptr::null())
        };
        if stop_event.is_null() {
            dbg("CP: listener stop event failed\n");
            return;
        }
        dbg("CP: listener starting\n");
        self.stop_event.store(stop_event as isize, Ordering::Release);
        let shared = Arc::clone(self);
        *listener = Some(thread::spawn(move || run_listener(shared)));
    }

    pub fn stop_listener(&self) {
        let handle = {
            let mut listener = self
                .listener
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            listener.take()
        };
        let stop_event = self.stop_event.swap(0, Ordering::AcqRel);
        if stop_event != 0 {
            unsafe {
                SetEvent(stop_event as HANDLE);
            }
        }
        if let Some(handle) = handle {
            let _ = handle.join();
        }
        if stop_event != 0 {
            unsafe {
                CloseHandle(stop_event as HANDLE);
            }
        }
    }
}

fn run_listener(shared: Arc<Shared>) {
    unsafe {
        let sddl = wide(SDDL);
        let mut descriptor: *mut c_void = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        ) == 0
        {
            dbg("PhoneKey CP: pipe SDDL failed\n");
            return;
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as DWORD,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let name = wide(PIPE_NAME);
        let stop_event = shared.stop_event.load(Ordering::Acquire) as HANDLE;

        while !stop_signaled(stop_event) {
            let pipe = CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                PIPE_BUFFER_SIZE as DWORD,
                PIPE_BUFFER_SIZE as DWORD,
                0,
                &attributes,
            );
            if pipe == INVALID_HANDLE_VALUE {
                dbg(&format!(
                    "CP: pipe create failed error={}\n",
                    GetLastError()
                ));
                if wait_stop(stop_event, 1000) {
                    break;
                }
                continue;
            }
            dbg("CP: pipe instance ready\n");

            let connect_event = CreateEventW(ptr::null(), 1, 0, ptr::null());
            if connect_event.is_null() {
                CloseHandle(pipe);
                continue;
            }
            let mut connect_overlap = OVERLAPPED::default();
            connect_overlap.hEvent = connect_event;
            let mut connected = ConnectNamedPipe(pipe, &mut connect_overlap) != 0;
            if !connected {
                let error = GetLastError();
                if error == ERROR_IO_PENDING {
                    let handles = [connect_event, stop_event];
                    let wait = WaitForMultipleObjects(2, handles.as_ptr(), 0, u32::MAX);
                    if wait != WAIT_OBJECT_0 {
                        CancelIo(pipe);
                        CloseHandle(connect_event);
                        CloseHandle(pipe);
                        continue;
                    }
                    let mut transferred = 0;
                    connected =
                        GetOverlappedResult(pipe, &mut connect_overlap, &mut transferred, 0) != 0;
                } else if error == ERROR_PIPE_CONNECTED {
                    connected = true;
                } else if !wait_stop(stop_event, 0) {
                    CloseHandle(connect_event);
                    CloseHandle(pipe);
                    continue;
                }
            }
            CloseHandle(connect_event);
            if !connected {
                CloseHandle(pipe);
                continue;
            }
            dbg("CP: pipe connected\n");

            let read_event = CreateEventW(ptr::null(), 1, 0, ptr::null());
            if read_event.is_null() {
                DisconnectNamedPipe(pipe);
                CloseHandle(pipe);
                continue;
            }
            let mut overlap = OVERLAPPED::default();
            overlap.hEvent = read_event;
            let mut buffer = [0u8; PIPE_BUFFER_SIZE];
            let mut read = 0u32;
            let mut read_ok = ReadFile(
                pipe,
                buffer.as_mut_ptr() as *mut c_void,
                (PIPE_BUFFER_SIZE - 1) as DWORD,
                &mut read,
                &mut overlap,
            ) != 0;
            if !read_ok {
                let error = GetLastError();
                if error == ERROR_IO_PENDING {
                    let handles = [read_event, stop_event];
                    let wait = WaitForMultipleObjects(
                        2,
                        handles.as_ptr(),
                        0,
                        PIPE_READ_TIMEOUT_MS,
                    );
                    if wait == WAIT_OBJECT_0 {
                        read_ok =
                            GetOverlappedResult(pipe, &mut overlap, &mut read, 0) != 0;
                    } else {
                        CancelIo(pipe);
                    }
                }
            }
            CloseHandle(read_event);

            if read_ok && !stop_signaled(stop_event) {
                let command = String::from_utf8_lossy(&buffer[..read as usize])
                    .trim_end_matches('\0')
                    .to_string();
                let tag: String = command
                    .chars()
                    .take_while(|value| *value != ':' && *value != '\\')
                    .take(16)
                    .collect();
                dbg(&format!("CP: command {tag}\n"));
                let response = handle_command(&shared, &command);
                write_response(pipe, response.as_bytes());
                FlushFileBuffers(pipe);
            }
            DisconnectNamedPipe(pipe);
            CloseHandle(pipe);
        }

        LocalFree(descriptor);
        dbg("PhoneKey CP: pipe listener stopped\n");
    }
}

fn handle_command(shared: &Shared, command: &str) -> &'static str {
    if command.starts_with("QUERY") {
        return shared.state_code();
    }
    if let Some(rest) = command.strip_prefix("STATUS:") {
        shared.set_nearby(rest.starts_with("NEAR"));
        return "OK";
    }
    if command.starts_with("CLEAR") {
        shared.clear_pending();
        return "OK";
    }
    if let Some(payload) = command.strip_prefix("UNLOCK:") {
        return match parse_unlock(payload) {
            Some((domain, username, password)) => {
                shared.set_credentials(domain, username, password);
                "OK"
            }
            None => "ERR:INVALID_CMD",
        };
    }
    "ERR:INVALID_CMD"
}

fn parse_unlock(payload: &str) -> Option<(String, String, String)> {
    let (user_part, password) = match payload.find('\\') {
        Some(backslash) => {
            let tail = &payload[backslash + 1..];
            let colon = tail.find(':')?;
            (
                Some((&payload[..backslash], &tail[..colon])),
                &tail[colon + 1..],
            )
        }
        None => {
            let colon = payload.find(':')?;
            (None, &payload[colon + 1..])
        }
    };
    let password = clean_field(password);
    let (domain, username) = match user_part {
        Some((domain, username)) => (clean_field(domain), clean_field(username)),
        None => {
            let colon = payload.find(':')?;
            (String::new(), clean_field(&payload[..colon]))
        }
    };
    Some((
        if domain.is_empty() {
            ".".to_string()
        } else {
            domain
        },
        username,
        password,
    ))
}

fn clean_field(value: &str) -> String {
    let value = value.split('\0').next().unwrap_or("");
    value.chars().take(255).collect()
}

unsafe fn write_response(pipe: HANDLE, response: &[u8]) {
    let event = CreateEventW(ptr::null(), 1, 0, ptr::null());
    if event.is_null() {
        return;
    }
    let mut overlap = OVERLAPPED::default();
    overlap.hEvent = event;
    let mut written = 0u32;
    let ok = WriteFile(
        pipe,
        response.as_ptr() as *const c_void,
        response.len() as DWORD,
        &mut written,
        &mut overlap,
    ) != 0;
    if !ok && GetLastError() == ERROR_IO_PENDING {
        if WaitForSingleObject(event, 2000) == WAIT_OBJECT_0 {
            GetOverlappedResult(pipe, &mut overlap, &mut written, 0);
        } else {
            CancelIo(pipe);
        }
    }
    CloseHandle(event);
}

pub fn verify_nearby(timeout_ms: u32) -> bool {
    let deadline = unsafe { GetTickCount64() } + timeout_ms as u64;
    let name = wide(VERIFY_PIPE_NAME);
    let pipe = loop {
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            break handle;
        }
        let error = unsafe { GetLastError() };
        if error != ERROR_PIPE_BUSY {
            return false;
        }
        let now = unsafe { GetTickCount64() };
        if now >= deadline {
            return false;
        }
        let remaining = (deadline - now).min(100) as u32;
        unsafe {
            WaitNamedPipeW(name.as_ptr(), remaining);
        }
    };

    let mut mode = PIPE_READMODE_MESSAGE;
    let mode_ok = unsafe {
        SetNamedPipeHandleState(pipe, &mut mode, ptr::null_mut(), ptr::null_mut())
    } != 0;
    if !mode_ok {
        unsafe {
            CloseHandle(pipe);
        }
        return false;
    }

    let write_ok = write_overlapped(pipe, b"VERIFY", deadline);
    let mut response = [0u8; 32];
    let read = if write_ok {
        read_overlapped(pipe, &mut response, deadline)
    } else {
        0
    };
    unsafe {
        CloseHandle(pipe);
    }
    read > 0 && response[..read].starts_with(b"OK")
}

pub fn fetch_credentials(timeout_ms: u32) -> CredentialFetch {
    let deadline = unsafe { GetTickCount64() } + timeout_ms as u64;
    let name = wide(CRED_PIPE_NAME);
    let pipe = loop {
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            break handle;
        }
        let error = unsafe { GetLastError() };
        if error != ERROR_PIPE_BUSY {
            dbg(&format!("CP: fetch open failed error={error}\n"));
            return CredentialFetch::Error;
        }
        let now = unsafe { GetTickCount64() };
        if now >= deadline {
            dbg("CP: fetch busy timeout\n");
            return CredentialFetch::Error;
        }
        let remaining = (deadline - now).min(100) as u32;
        unsafe {
            WaitNamedPipeW(name.as_ptr(), remaining);
        }
    };

    let mut mode = PIPE_READMODE_MESSAGE;
    let mode_ok = unsafe {
        SetNamedPipeHandleState(pipe, &mut mode, ptr::null_mut(), ptr::null_mut())
    } != 0;
    if !mode_ok {
        dbg("CP: fetch message mode failed\n");
        unsafe {
            CloseHandle(pipe);
        }
        return CredentialFetch::Error;
    }
    dbg("CP: fetch connected\n");

    let mut response = [0u8; 4096];
    let read_event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
    if read_event.is_null() {
        dbg("CP: fetch read event failed\n");
        unsafe {
            CloseHandle(pipe);
        }
        return CredentialFetch::Error;
    }
    let mut read_overlap = OVERLAPPED::default();
    read_overlap.hEvent = read_event;
    let mut immediate_read = 0u32;
    // Arm the read before sending GET: the app disconnects right after
    // writing the response, discarding data no pending read has claimed.
    let read_immediate = unsafe {
        ReadFile(
            pipe,
            response.as_mut_ptr() as *mut c_void,
            response.len() as DWORD,
            &mut immediate_read,
            &mut read_overlap,
        ) != 0
    };
    let read_error = if read_immediate {
        0
    } else {
        unsafe { GetLastError() }
    };
    if !read_immediate && read_error != ERROR_IO_PENDING {
        dbg(&format!("CP: fetch read failed error={read_error}\n"));
        unsafe {
            CloseHandle(read_event);
            CloseHandle(pipe);
        }
        return CredentialFetch::Error;
    }

    let write_ok = write_overlapped(pipe, b"GET", deadline);
    let read = if write_ok {
        unsafe {
            finish_pending_read(
                pipe,
                read_event,
                &mut read_overlap,
                read_immediate,
                read_error,
                immediate_read,
                response.len(),
                deadline,
            )
        }
    } else {
        unsafe {
            CancelIo(pipe);
            let mut ignored = 0u32;
            let _ = GetOverlappedResult(pipe, &mut read_overlap, &mut ignored, 1);
            CloseHandle(read_event);
        }
        0
    };
    unsafe {
        CloseHandle(pipe);
    }
    dbg(&format!("CP: fetch write={write_ok} read={read}\n"));
    if read == 0 {
        return CredentialFetch::Error;
    }

    let result = parse_credential_response(&response[..read]);
    let tag = match &result {
        CredentialFetch::Ok { .. } => "OK",
        CredentialFetch::Deny => "DENY",
        CredentialFetch::Off => "OFF",
        CredentialFetch::NoCred => "NOCRED",
        CredentialFetch::Error => "ERROR",
    };
    dbg(&format!("CP: fetch result={tag}\n"));
    unsafe {
        ptr::write_bytes(response.as_mut_ptr(), 0, response.len());
    }
    result
}

fn parse_credential_response(data: &[u8]) -> CredentialFetch {
    if data.starts_with(b"DENY") {
        return CredentialFetch::Deny;
    }
    if data.starts_with(b"OFF") {
        return CredentialFetch::Off;
    }
    if data.starts_with(b"NOCRED") {
        return CredentialFetch::NoCred;
    }
    let mut parts = data.split(|byte| *byte == 0);
    if parts.next() != Some(b"OK".as_slice()) {
        return CredentialFetch::Error;
    }
    let (Some(domain), Some(username), Some(password)) = (parts.next(), parts.next(), parts.next())
    else {
        return CredentialFetch::Error;
    };
    if username.is_empty() || password.is_empty() {
        return CredentialFetch::NoCred;
    }
    let Ok(domain) = std::str::from_utf8(domain) else {
        return CredentialFetch::Error;
    };
    let Ok(username) = std::str::from_utf8(username) else {
        return CredentialFetch::Error;
    };
    let Ok(password) = std::str::from_utf8(password) else {
        return CredentialFetch::Error;
    };
    CredentialFetch::Ok {
        domain: domain.to_string(),
        username: username.to_string(),
        password: password.to_string(),
    }
}

fn write_overlapped(pipe: HANDLE, data: &[u8], deadline: u64) -> bool {
    unsafe {
        let event = CreateEventW(ptr::null(), 1, 0, ptr::null());
        if event.is_null() {
            return false;
        }
        let mut overlap = OVERLAPPED::default();
        overlap.hEvent = event;
        let mut written = 0u32;
        let ok = WriteFile(
            pipe,
            data.as_ptr() as *const c_void,
            data.len() as DWORD,
            &mut written,
            &mut overlap,
        ) != 0;
        let result = finish_overlapped(pipe, event, &mut overlap, ok, deadline);
        CloseHandle(event);
        result
    }
}

unsafe fn finish_pending_read(
    pipe: HANDLE,
    event: HANDLE,
    overlap: &mut OVERLAPPED,
    immediate: bool,
    error: u32,
    immediate_read: u32,
    buffer_len: usize,
    deadline: u64,
) -> usize {
    if immediate {
        CloseHandle(event);
        return (immediate_read as usize).min(buffer_len);
    }
    if error != ERROR_IO_PENDING {
        dbg(&format!("CP: fetch read failed error={error}\n"));
        CloseHandle(event);
        return 0;
    }
    let now = GetTickCount64();
    if now >= deadline {
        dbg("CP: fetch read timeout\n");
        CancelIo(pipe);
        CloseHandle(event);
        return 0;
    }
    let wait = WaitForSingleObject(event, (deadline - now) as u32);
    if wait != WAIT_OBJECT_0 {
        dbg("CP: fetch read wait failed\n");
        CancelIo(pipe);
        CloseHandle(event);
        return 0;
    }
    let mut transferred = 0;
    let ok = GetOverlappedResult(pipe, overlap, &mut transferred, 0) != 0;
    if !ok {
        dbg(&format!(
            "CP: fetch read result failed error={}\n",
            GetLastError()
        ));
    }
    CloseHandle(event);
    if ok {
        (transferred as usize).min(buffer_len)
    } else {
        0
    }
}

fn read_overlapped(pipe: HANDLE, buffer: &mut [u8], deadline: u64) -> usize {
    unsafe {
        let event = CreateEventW(ptr::null(), 1, 0, ptr::null());
        if event.is_null() {
            return 0;
        }
        let mut overlap = OVERLAPPED::default();
        overlap.hEvent = event;
        let mut read = 0u32;
        let ok = ReadFile(
            pipe,
            buffer.as_mut_ptr() as *mut c_void,
            buffer.len() as DWORD,
            &mut read,
            &mut overlap,
        ) != 0;
        let result = finish_overlapped(pipe, event, &mut overlap, ok, deadline);
        CloseHandle(event);
        if result && read <= buffer.len() as u32 {
            read as usize
        } else {
            0
        }
    }
}

unsafe fn finish_overlapped(
    pipe: HANDLE,
    event: HANDLE,
    overlap: &mut OVERLAPPED,
    immediate: bool,
    deadline: u64,
) -> bool {
    if immediate {
        return true;
    }
    if GetLastError() != ERROR_IO_PENDING {
        return false;
    }
    let now = GetTickCount64();
    if now >= deadline {
        CancelIo(pipe);
        return false;
    }
    let wait = WaitForSingleObject(event, (deadline - now) as u32);
    if wait != WAIT_OBJECT_0 {
        CancelIo(pipe);
        return false;
    }
    let mut transferred = 0;
    GetOverlappedResult(pipe, overlap, &mut transferred, 0) != 0
}

unsafe fn stop_signaled(stop_event: HANDLE) -> bool {
    stop_event.is_null() || WaitForSingleObject(stop_event, 0) == WAIT_OBJECT_0
}

unsafe fn wait_stop(stop_event: HANDLE, timeout_ms: u32) -> bool {
    stop_event.is_null() || WaitForSingleObject(stop_event, timeout_ms) == WAIT_OBJECT_0
}

fn clip(value: String) -> String {
    value.chars().take(255).collect()
}

fn wipe(value: &mut String) {
    if !value.is_empty() {
        unsafe {
            ptr::write_bytes(value.as_mut_ptr(), 0, value.len());
        }
        value.clear();
    }
}

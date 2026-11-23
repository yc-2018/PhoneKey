#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::c_void;

pub type HANDLE = *mut c_void;
pub type HINSTANCE = *mut c_void;
pub type HMODULE = *mut c_void;
pub type HWND = isize;
pub type BOOL = i32;
pub type DWORD = u32;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type WNDPROC =
    Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> isize>;

pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
pub const TRUE: BOOL = 1;

pub const PIPE_ACCESS_DUPLEX: DWORD = 0x0000_0003;
pub const FILE_FLAG_OVERLAPPED: DWORD = 0x4000_0000;
pub const FILE_FLAG_FIRST_PIPE_INSTANCE: DWORD = 0x0008_0000;
pub const PIPE_TYPE_MESSAGE: DWORD = 0x0000_0004;
pub const PIPE_READMODE_MESSAGE: DWORD = 0x0000_0002;
pub const PIPE_WAIT: DWORD = 0x0000_0000;
pub const PIPE_REJECT_REMOTE_CLIENTS: DWORD = 0x0000_0008;

pub const GENERIC_READ: DWORD = 0x8000_0000;
pub const GENERIC_WRITE: DWORD = 0x4000_0000;
pub const OPEN_EXISTING: DWORD = 3;

pub const ERROR_ACCESS_DENIED: DWORD = 5;
pub const ERROR_PIPE_BUSY: DWORD = 231;
pub const ERROR_IO_PENDING: DWORD = 997;
pub const ERROR_PIPE_CONNECTED: DWORD = 535;
pub const ERROR_ALREADY_EXISTS: DWORD = 183;

pub const WAIT_OBJECT_0: DWORD = 0;
pub const WAIT_TIMEOUT: DWORD = 258;

pub const WM_NCCREATE: u32 = 0x0081;
pub const WM_APP: u32 = 0x8000;
pub const HWND_MESSAGE: HWND = -3;
pub const GWLP_USERDATA: i32 = -21;

#[repr(C)]
pub struct SECURITY_ATTRIBUTES {
    pub nLength: DWORD,
    pub lpSecurityDescriptor: *mut c_void,
    pub bInheritHandle: BOOL,
}

#[repr(C)]
pub struct OVERLAPPED {
    pub Internal: usize,
    pub InternalHigh: usize,
    pub Offset: u32,
    pub OffsetHigh: u32,
    pub hEvent: HANDLE,
}

impl Default for OVERLAPPED {
    fn default() -> Self {
        Self {
            Internal: 0,
            InternalHigh: 0,
            Offset: 0,
            OffsetHigh: 0,
            hEvent: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
pub struct WNDCLASSW {
    pub style: u32,
    pub lpfnWndProc: WNDPROC,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: HANDLE,
    pub hCursor: HANDLE,
    pub hbrBackground: HANDLE,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
}

impl Default for WNDCLASSW {
    fn default() -> Self {
        Self {
            style: 0,
            lpfnWndProc: None,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: std::ptr::null_mut(),
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: std::ptr::null(),
        }
    }
}

#[repr(C)]
pub struct CREATESTRUCTW {
    pub lpCreateParams: *mut c_void,
    pub hInstance: HINSTANCE,
    pub hMenu: HANDLE,
    pub hwndParent: HWND,
    pub cy: i32,
    pub cx: i32,
    pub y: i32,
    pub x: i32,
    pub style: i32,
    pub lpszName: *const u16,
    pub lpszClass: *const u16,
    pub dwExStyle: u32,
}

#[link(name = "kernel32")]
extern "system" {
    pub fn CreateNamedPipeW(
        lpName: *const u16,
        dwOpenMode: DWORD,
        dwPipeMode: DWORD,
        nMaxInstances: DWORD,
        nOutBufferSize: DWORD,
        nInBufferSize: DWORD,
        nDefaultTimeOut: DWORD,
        lpSecurityAttributes: *const SECURITY_ATTRIBUTES,
    ) -> HANDLE;
    pub fn ConnectNamedPipe(hNamedPipe: HANDLE, lpOverlapped: *mut OVERLAPPED) -> BOOL;
    pub fn DisconnectNamedPipe(hNamedPipe: HANDLE) -> BOOL;
    pub fn ReadFile(
        hFile: HANDLE,
        lpBuffer: *mut c_void,
        nNumberOfBytesToRead: DWORD,
        lpNumberOfBytesRead: *mut DWORD,
        lpOverlapped: *mut OVERLAPPED,
    ) -> BOOL;
    pub fn WriteFile(
        hFile: HANDLE,
        lpBuffer: *const c_void,
        nNumberOfBytesToWrite: DWORD,
        lpNumberOfBytesWritten: *mut DWORD,
        lpOverlapped: *mut OVERLAPPED,
    ) -> BOOL;
    pub fn FlushFileBuffers(hFile: HANDLE) -> BOOL;
    pub fn CloseHandle(hObject: HANDLE) -> BOOL;
    pub fn CreateEventW(
        lpEventAttributes: *const SECURITY_ATTRIBUTES,
        bManualReset: BOOL,
        bInitialState: BOOL,
        lpName: *const u16,
    ) -> HANDLE;
    pub fn SetEvent(hEvent: HANDLE) -> BOOL;
    pub fn WaitForSingleObject(hHandle: HANDLE, dwMilliseconds: DWORD) -> DWORD;
    pub fn WaitForMultipleObjects(
        nCount: DWORD,
        lpHandles: *const HANDLE,
        bWaitAll: BOOL,
        dwMilliseconds: DWORD,
    ) -> DWORD;
    pub fn CancelIo(hFile: HANDLE) -> BOOL;
    pub fn GetOverlappedResult(
        hFile: HANDLE,
        lpOverlapped: *mut OVERLAPPED,
        lpNumberOfBytesTransferred: *mut DWORD,
        bWait: BOOL,
    ) -> BOOL;
    pub fn CreateFileW(
        lpFileName: *const u16,
        dwDesiredAccess: DWORD,
        dwShareMode: DWORD,
        lpSecurityAttributes: *const SECURITY_ATTRIBUTES,
        dwCreationDisposition: DWORD,
        dwFlagsAndAttributes: DWORD,
        hTemplateFile: HANDLE,
    ) -> HANDLE;
    pub fn WaitNamedPipeW(lpNamedPipeName: *const u16, nTimeOut: DWORD) -> BOOL;
    pub fn SetNamedPipeHandleState(
        hNamedPipe: HANDLE,
        lpMode: *mut DWORD,
        lpMaxCollectionCount: *mut DWORD,
        lpCollectDataTimeout: *mut DWORD,
    ) -> BOOL;
    pub fn GetLastError() -> DWORD;
    pub fn GetTickCount64() -> u64;
    pub fn LocalFree(hMem: *mut c_void) -> *mut c_void;
    pub fn GetModuleHandleW(lpModuleName: *const u16) -> HMODULE;
}

#[link(name = "advapi32")]
extern "system" {
    pub fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        StringSecurityDescriptor: *const u16,
        StringSDRevision: DWORD,
        SecurityDescriptor: *mut *mut c_void,
        SecurityDescriptorSize: *mut DWORD,
    ) -> BOOL;
}

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassW(lpWndClass: *const WNDCLASSW) -> u16;
    pub fn UnregisterClassW(
        lpClassName: *const u16,
        hInstance: HINSTANCE,
    ) -> BOOL;
    pub fn CreateWindowExW(
        dwExStyle: DWORD,
        lpClassName: *const u16,
        lpWindowName: *const u16,
        dwStyle: DWORD,
        X: i32,
        Y: i32,
        nWidth: i32,
        nHeight: i32,
        hWndParent: HWND,
        hMenu: HANDLE,
        hInstance: HINSTANCE,
        lpParam: *mut c_void,
    ) -> HWND;
    pub fn DestroyWindow(hWnd: HWND) -> BOOL;
    pub fn DefWindowProcW(
        hWnd: HWND,
        Msg: u32,
        wParam: WPARAM,
        lParam: LPARAM,
    ) -> LPARAM;
    pub fn SetWindowLongPtrW(
        hWnd: HWND,
        nIndex: i32,
        dwNewLong: isize,
    ) -> isize;
    pub fn GetWindowLongPtrW(hWnd: HWND, nIndex: i32) -> isize;
    pub fn PostMessageW(
        hWnd: HWND,
        Msg: u32,
        wParam: WPARAM,
        lParam: LPARAM,
    ) -> BOOL;
}

pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn dbg(message: &str) {
    #[link(name = "kernel32")]
    extern "system" {
        fn OutputDebugStringW(lpOutputString: *const u16);
    }
    let text = wide(message);
    unsafe { OutputDebugStringW(text.as_ptr()) };
    log_to_file(message);
}

fn log_to_file(message: &str) {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    let directory = std::path::Path::new(r"C:\ProgramData\PhoneKey");
    if std::fs::create_dir_all(directory).is_err() {
        return;
    }
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("cp.log"))
    else {
        return;
    };
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0);
    let _ = writeln!(
        file,
        "{millis} pid={} {}",
        std::process::id(),
        message.trim_end()
    );
}

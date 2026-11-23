use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use log::{info, warn};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Power::{
    PowerRegisterSuspendResumeNotification, PowerUnregisterSuspendResumeNotification,
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND,
};

unsafe extern "system" fn resume_callback(
    context: *const c_void,
    event_type: u32,
    _setting: *const c_void,
) -> u32 {
    if matches!(event_type, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND) && !context.is_null()
    {
        let signal = &*(context as *const AtomicBool);
        signal.store(true, Ordering::Release);
    }
    0
}

pub struct ResumeWatcher {
    handle: Option<HPOWERNOTIFY>,
    _params: Box<DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS>,
    _signal: Arc<AtomicBool>,
}

// 回调只在系统线程里读 Arc<AtomicBool>；参数框地址固定，随 watcher 一起移动。
unsafe impl Send for ResumeWatcher {}

impl ResumeWatcher {
    pub fn start(signal: Arc<AtomicBool>) -> Self {
        let context = Arc::as_ptr(&signal) as *mut c_void;
        let params = Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(resume_callback),
            Context: context,
        });
        let mut raw: *mut c_void = ptr::null_mut();
        let result = unsafe {
            PowerRegisterSuspendResumeNotification(
                DEVICE_NOTIFY_CALLBACK,
                HANDLE(&*params as *const _ as *mut c_void),
                &mut raw,
            )
        };
        if result.0 != 0 {
            warn!("注册系统唤醒通知失败（错误 {}）", result.0);
            return Self {
                handle: None,
                _params: params,
                _signal: signal,
            };
        }
        info!("系统唤醒通知已注册");
        Self {
            handle: Some(HPOWERNOTIFY(raw as isize)),
            _params: params,
            _signal: signal,
        }
    }
}

impl Drop for ResumeWatcher {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            unsafe {
                let _ = PowerUnregisterSuspendResumeNotification(handle);
            }
        }
    }
}

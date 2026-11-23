use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::config::{save_settings, DeviceBinding, Settings};
use crate::power::ResumeWatcher;
use crate::presence::{
    DiscoveredDevice, PresenceMonitor, PresenceSnapshot, STATE_AWAY, STATE_NEAR, STATE_UNKNOWN,
};
use crate::secret;
use crate::sys;
use crate::unlock;
use crate::verify_pipe::{CredentialServer, VerifyServer};

const CP_CACHE_SECONDS: f64 = 5.0;
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(300);
const RESUME_DEBOUNCE_SECONDS: f64 = 5.0;

#[derive(Serialize, Clone)]
pub struct Snapshot {
    pub presence: PresenceSnapshot,
    pub settings: Settings,
    pub locked: bool,
    pub remote: bool,
    pub paused: bool,
    pub account: String,
    pub domain: String,
    pub has_password: bool,
    pub cp_installed: bool,
    pub last_action: String,
    pub last_error: String,
    pub unlock_offered: bool,
    pub lock_check: Option<String>,
}

struct Inner {
    locked: bool,
    remote: bool,
    presence_state: String,
    lock_check: Option<String>,
    pending_lock_at: Option<Instant>,
    offer_at: Option<Instant>,
    offered: bool,
    last_action: String,
    last_error: String,
    cp_checked: Option<Instant>,
    cp_value: bool,
    last_resume: Option<Instant>,
}

impl Inner {
    fn new(locked: bool, remote: bool) -> Self {
        Self {
            locked,
            remote,
            presence_state: STATE_UNKNOWN.to_string(),
            lock_check: None,
            pending_lock_at: None,
            offer_at: None,
            offered: false,
            last_action: "已就绪".to_string(),
            last_error: String::new(),
            cp_checked: None,
            cp_value: false,
            last_resume: None,
        }
    }
}

pub struct Controller {
    settings: Arc<RwLock<Settings>>,
    presence: Arc<PresenceMonitor>,
    inner: Mutex<Inner>,
    stop: Arc<AtomicBool>,
    resume_signal: Arc<AtomicBool>,
    verify: Mutex<Option<VerifyServer>>,
    credential: Mutex<Option<CredentialServer>>,
    resume: Mutex<Option<ResumeWatcher>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl Controller {
    pub fn new(settings: Arc<RwLock<Settings>>, presence: Arc<PresenceMonitor>) -> Arc<Self> {
        Arc::new(Self {
            settings,
            presence,
            inner: Mutex::new(Inner::new(sys::is_locked(), sys::is_remote_session())),
            stop: Arc::new(AtomicBool::new(false)),
            resume_signal: Arc::new(AtomicBool::new(false)),
            verify: Mutex::new(None),
            credential: Mutex::new(None),
            resume: Mutex::new(None),
            threads: Mutex::new(Vec::new()),
        })
    }

    pub fn start(self: &Arc<Self>) {
        if let Ok(mut guard) = self.verify.lock() {
            if guard.is_none() {
                *guard = Some(VerifyServer::start(self.presence.clone()));
            }
        }
        if let Ok(mut guard) = self.credential.lock() {
            if guard.is_none() {
                *guard = Some(CredentialServer::start(
                    self.presence.clone(),
                    self.settings.clone(),
                ));
            }
        }
        if let Ok(mut guard) = self.resume.lock() {
            if guard.is_none() {
                *guard = Some(ResumeWatcher::start(self.resume_signal.clone()));
            }
        }

        let worker = self.clone();
        match thread::Builder::new()
            .name("auto-controller".to_string())
            .spawn(move || worker.work_loop())
        {
            Ok(handle) => {
                if let Ok(mut threads) = self.threads.lock() {
                    threads.push(handle);
                }
            }
            Err(error) => log::error!("启动自动控制线程失败: {error}"),
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.withdraw_offer("应用退出");
        if let Ok(mut guard) = self.verify.lock() {
            if let Some(mut server) = guard.take() {
                server.stop();
            }
        }
        if let Ok(mut guard) = self.credential.lock() {
            if let Some(mut server) = guard.take() {
                server.stop();
            }
        }
        if let Ok(mut guard) = self.resume.lock() {
            guard.take();
        }
        self.presence.stop();
        if let Ok(mut threads) = self.threads.lock() {
            while let Some(handle) = threads.pop() {
                let _ = handle.join();
            }
        }
    }

    pub fn settings_handle(&self) -> Arc<RwLock<Settings>> {
        self.settings.clone()
    }

    fn settings_snapshot(&self) -> Settings {
        self.settings
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    fn set_action(&self, action: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.last_action = action.to_string();
            inner.last_error.clear();
        }
    }

    fn set_error(&self, message: &str) {
        log::warn!("{message}");
        if let Ok(mut inner) = self.inner.lock() {
            inner.last_error = message.to_string();
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let presence = self.presence.snapshot();
        let settings = self.settings_snapshot();
        let (locked, remote, action, error, offered, lock_check) = {
            let inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
            (
                inner.locked,
                inner.remote,
                inner.last_action.clone(),
                inner.last_error.clone(),
                inner.offered,
                inner.lock_check.clone(),
            )
        };
        Snapshot {
            presence,
            paused: settings.paused,
            settings,
            locked,
            remote,
            account: sys::account_name(),
            domain: sys::account_domain(),
            has_password: secret::has_secret(),
            cp_installed: self.cp_installed(),
            last_action: action,
            last_error: error,
            unlock_offered: offered,
            lock_check,
        }
    }

    pub fn cp_installed(&self) -> bool {
        {
            let inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
            if let Some(checked) = inner.cp_checked {
                if checked.elapsed().as_secs_f64() < CP_CACHE_SECONDS {
                    return inner.cp_value;
                }
            }
        }
        let value = unlock::cp_installed();
        if let Ok(mut inner) = self.inner.lock() {
            inner.cp_value = value;
            inner.cp_checked = Some(Instant::now());
        }
        value
    }

    pub fn invalidate_cp_cache(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.cp_checked = None;
        }
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), String> {
        {
            let mut settings = self
                .settings
                .write()
                .unwrap_or_else(|poison| poison.into_inner());
            settings.paused = paused;
            save_settings(&mut settings);
        }
        if paused {
            if let Ok(mut inner) = self.inner.lock() {
                inner.pending_lock_at = None;
                inner.offer_at = None;
                inner.last_action = "已暂停自动锁屏与手机解锁".to_string();
            }
            log::info!("自动操作已暂停");
        } else {
            self.presence.reset();
            if let Ok(mut inner) = self.inner.lock() {
                inner.last_action = "已恢复自动操作".to_string();
                inner.last_error.clear();
            }
            log::info!("自动操作已恢复");
        }
        Ok(())
    }

    pub fn lock_now(&self) -> Result<(), String> {
        if sys::lock_workstation() {
            log::info!("手动触发锁屏");
            Ok(())
        } else {
            self.set_error("锁屏失败");
            Err("锁屏失败".to_string())
        }
    }

    pub fn refresh_presence(&self) {
        self.withdraw_offer("手动刷新");
        self.presence.refresh();
        self.set_action("正在重新检测手机…");
        log::info!("手动刷新手机检测");
    }

    pub fn rebind_phone(&self) {
        {
            let mut settings = self
                .settings
                .write()
                .unwrap_or_else(|poison| poison.into_inner());
            settings.phone_adv_enabled = true;
            settings.bound_devices.clear();
            save_settings(&mut settings);
        }
        self.presence.rebind();
        if let Ok(mut inner) = self.inner.lock() {
            inner.lock_check = None;
            inner.last_action = "正在自动识别手机广播…".to_string();
            inner.last_error.clear();
        }
        log::info!("已清除设备绑定，开始自动识别");
    }

    pub fn apply_device_bindings(
        &self,
        devices: Vec<DeviceBinding>,
        policy: &str,
    ) -> Result<(), String> {
        let (count, condition) = {
            let mut settings = self
                .settings
                .write()
                .unwrap_or_else(|poison| poison.into_inner());
            settings.device_policy = policy.to_string();
            settings.bound_devices = devices;
            settings.normalize();
            save_settings(&mut settings);
            (
                settings.bound_devices.len(),
                if settings.device_policy == "all" {
                    "全部在附近"
                } else {
                    "任一在附近"
                },
            )
        };
        self.withdraw_offer("解锁设备已更新");
        self.presence.rebind();
        if let Ok(mut inner) = self.inner.lock() {
            inner.lock_check = None;
            inner.last_action = format!("已更新解锁设备：{count} 台，{condition}");
            inner.last_error.clear();
        }
        log::info!("已更新解锁设备：{count} 台，策略 {condition}");
        Ok(())
    }

    pub fn set_device_policy(&self, policy: &str) -> Result<(), String> {
        let condition = {
            let mut settings = self
                .settings
                .write()
                .unwrap_or_else(|poison| poison.into_inner());
            settings.device_policy = policy.to_string();
            settings.normalize();
            save_settings(&mut settings);
            if settings.device_policy == "all" {
                "全部在附近"
            } else {
                "任一在附近"
            }
        };
        self.withdraw_offer("解锁策略已更新");
        self.presence.rebind();
        self.set_action(&format!("已更新解锁条件：{condition}"));
        Ok(())
    }

    pub fn update_settings(&self, mut updated: Settings) -> Result<(), String> {
        updated.normalize();
        updated.paused = self.settings_snapshot().paused;
        {
            let mut settings = self
                .settings
                .write()
                .unwrap_or_else(|poison| poison.into_inner());
            let autostart = settings.autostart;
            settings.clone_from(&updated);
            settings.autostart = autostart;
            save_settings(&mut settings);
        }
        if !updated.click_unlock {
            self.withdraw_offer("手机解锁已关闭");
        }
        Ok(())
    }

    pub fn save_password(&self, password: &str) -> Result<(), String> {
        if password.is_empty() {
            return Err("密码不能为空".to_string());
        }
        let username = sys::account_name();
        let domain = sys::account_domain();
        if username.is_empty() {
            return Err("无法读取当前 Windows 账户".to_string());
        }
        sys::verify_credentials(&username, &domain, password)?;
        secret::save_secret(password)?;
        self.withdraw_offer("密码已更新");
        self.set_action("Windows 密码已加密保存");
        log::info!("已保存 Windows 密码");
        Ok(())
    }

    pub fn clear_password(&self) -> Result<(), String> {
        self.withdraw_offer("密码已清除");
        secret::clear_secret()?;
        self.set_action("已清除保存的 Windows 密码");
        log::info!("已清除保存的 Windows 密码");
        Ok(())
    }

    pub fn set_autostart(&self, enabled: bool) -> Result<(), String> {
        crate::autostart::set_enabled(enabled)?;
        {
            let mut settings = self
                .settings
                .write()
                .unwrap_or_else(|poison| poison.into_inner());
            settings.autostart = enabled;
            save_settings(&mut settings);
        }
        if enabled {
            self.set_action("已开启开机自启");
        } else {
            self.set_action("已关闭开机自启");
        }
        Ok(())
    }

    pub fn list_discovered(&self, max_age: f64) -> Vec<DiscoveredDevice> {
        self.presence.list_discovered(max_age)
    }

    fn work_loop(self: Arc<Self>) {
        while !self.stop.load(Ordering::SeqCst) {
            if self.resume_signal.swap(false, Ordering::AcqRel) {
                self.handle_resume();
            }

            let locked = sys::is_locked();
            let remote = sys::is_remote_session();
            self.update_lock_state(locked, remote);
            self.update_presence_state();
            self.evaluate();
            thread::sleep(LOCK_POLL_INTERVAL);
        }
    }

    fn update_lock_state(&self, locked: bool, remote: bool) {
        {
            let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
            if locked != inner.locked {
                inner.locked = locked;
                inner.pending_lock_at = None;
                inner.offer_at = None;
                if locked {
                    inner.offered = false;
                    let near = self.presence.check_near(None);
                    inner.lock_check = Some(if near {
                        STATE_NEAR.to_string()
                    } else {
                        STATE_AWAY.to_string()
                    });
                    inner.last_action = "屏幕已锁定".to_string();
                    log::info!(
                        "进入锁屏，检测手机：{}",
                        if near { "在附近" } else { "不在附近" }
                    );
                } else {
                    inner.offered = false;
                    inner.lock_check = None;
                    inner.last_action = "屏幕已解锁".to_string();
                    inner.last_error.clear();
                    log::info!("检测到解锁");
                }
            }
            if remote != inner.remote {
                inner.remote = remote;
                if remote {
                    inner.pending_lock_at = None;
                    inner.offer_at = None;
                    log::info!("检测到远程桌面会话，自动操作暂停");
                } else {
                    log::info!("远程桌面会话结束");
                }
            }
        }
    }

    fn update_presence_state(&self) {
        let state = self.presence.snapshot().state;
        let settings = self.settings_snapshot();
        let near = self.presence.check_near(None);
        {
            let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
            if state != inner.presence_state {
                inner.presence_state = state.clone();
                if state == STATE_NEAR {
                    inner.pending_lock_at = None;
                    if inner.locked && settings.click_unlock {
                        inner.last_action = "手机在附近 · 点击解锁时仍需检测".to_string();
                    }
                } else if state == STATE_AWAY {
                    inner.pending_lock_at = None;
                    if inner.locked && settings.click_unlock {
                        inner.last_action = "手机已离开 · 点击解锁时会重新检测".to_string();
                    }
                }
            }
            if inner.locked {
                let next = if near { STATE_NEAR } else { STATE_AWAY };
                if inner.lock_check.as_deref() != Some(next) {
                    inner.lock_check = Some(next.to_string());
                }
            }
        }
    }

    fn evaluate(&self) {
        let settings = self.settings_snapshot();
        let presence = self.presence.snapshot();
        let cp_ready = self.cp_installed();
        let now = Instant::now();
        let mut action: Option<&str> = None;

        {
            let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
            let scan_ok = presence.scanning && presence.error.is_none();
            let active = !settings.paused && !inner.remote;

            if !active {
                inner.pending_lock_at = None;
                inner.offer_at = None;
                if inner.offered {
                    action = Some("withdraw");
                }
            } else {
                if settings.auto_lock
                    && scan_ok
                    && !inner.locked
                    && presence.state == STATE_AWAY
                    && presence.seen_since_start
                {
                    if inner.pending_lock_at.is_none() {
                        inner.pending_lock_at =
                            Some(now + Duration::from_secs_f64(settings.lock_grace));
                    }
                } else if inner.pending_lock_at.is_some()
                    && (inner.locked || presence.state != STATE_AWAY || !scan_ok)
                {
                    inner.pending_lock_at = None;
                }

                if let Some(target) = inner.pending_lock_at {
                    if now >= target {
                        inner.pending_lock_at = None;
                        action = Some("lock");
                    }
                }

                let ready = settings.click_unlock
                    && inner.locked
                    && secret::has_secret()
                    && cp_ready;
                if !ready {
                    inner.offer_at = None;
                    if inner.offered && action.is_none() {
                        action = Some("withdraw");
                    }
                } else if !inner.offered {
                    if inner.offer_at.is_none() {
                        inner.offer_at =
                            Some(now + Duration::from_secs_f64(settings.unlock_delay));
                    } else if now >= inner.offer_at.unwrap() {
                        inner.offer_at = None;
                        if action.is_none() {
                            action = Some("offer");
                        }
                    }
                }
            }
        }

        match action {
            Some("lock") => {
                if sys::lock_workstation() {
                    if let Ok(mut inner) = self.inner.lock() {
                        inner.locked = true;
                        inner.pending_lock_at = None;
                        inner.offer_at = None;
                        inner.offered = false;
                        inner.last_action = "手机已远离，自动锁屏".to_string();
                    }
                    log::info!("手机已远离，执行自动锁屏");
                } else {
                    self.set_error("自动锁屏失败");
                }
            }
            Some("offer") => self.offer_unlock(),
            Some("withdraw") => self.withdraw_offer("自动操作已关闭"),
            _ => {}
        }
    }

    fn offer_unlock(&self) {
        let still_ready = {
            let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
            inner.offered = true;
            inner.offer_at = None;
            let settings = self.settings_snapshot();
            let ready = !self.stop.load(Ordering::SeqCst)
                && settings.click_unlock
                && !settings.paused
                && !inner.remote
                && inner.locked;
            if ready {
                inner.last_action = "锁屏已就绪 · 点击磁贴时检测手机".to_string();
                inner.last_error.clear();
            }
            ready
        };

        if !still_ready {
            self.withdraw_offer("状态已变化");
            return;
        }
        log::info!("锁屏手机解锁已就绪，点击磁贴时检测手机");
    }

    fn withdraw_offer(&self, _reason: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        inner.offered = false;
        inner.offer_at = None;
    }

    fn handle_resume(&self) {
        if self.stop.load(Ordering::SeqCst) {
            return;
        }
        {
            let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
            if let Some(last) = inner.last_resume {
                if last.elapsed().as_secs_f64() < RESUME_DEBOUNCE_SECONDS {
                    return;
                }
            }
            inner.last_resume = Some(Instant::now());
            inner.offered = false;
            inner.offer_at = None;
            inner.last_action = "系统已唤醒，正在重新扫描手机".to_string();
            inner.last_error.clear();
        }
        log::info!("系统已从睡眠/休眠恢复，重新启动 BLE 扫描");
        self.presence.refresh();
    }
}

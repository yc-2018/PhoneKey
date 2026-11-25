mod autostart;
mod ble;
mod config;
mod controller;
mod cpsetup;
mod paths;
mod power;
mod presence;
mod secret;
mod sys;
mod unlock;
mod verify_pipe;
mod wifi;

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONWARNING, MB_YESNO, SW_SHOWNORMAL,
};

use config::{DeviceBinding, Settings};
use controller::{Controller, Snapshot};
use presence::{DiscoveredDevice, PresenceMonitor};

const TRAY_ID: &str = "phonekey";

struct ManagedState {
    controller: Arc<Controller>,
    resource_dir: Option<PathBuf>,
}

fn apply_tray_visibility(app: &tauri::AppHandle, hide_mode: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let visible = hide_mode != "hidden";
        if let Err(error) = tray.set_visible(visible) {
            log::warn!("设置托盘图标可见性失败: {error}");
        }
    }
}

fn confirm_quit() -> bool {
    let message = "退出 PhoneKey？\n\n\
        退出后将停止手机检测，锁屏界面不能再使用手机感应解锁。\n\
        重新启动 PhoneKey 后可恢复；PIN 或密码仍可正常登录。";
    let message: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    let caption: Vec<u16> = "PhoneKey"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(caption.as_ptr()),
            MB_YESNO | MB_ICONWARNING,
        ) == IDYES
    }
}

fn init_logging() {
    let Ok(file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths::log_path())
    else {
        return;
    };
    let config = simplelog::ConfigBuilder::new()
        .set_time_format_rfc3339()
        .build();
    let _ = simplelog::WriteLogger::init(simplelog::LevelFilter::Info, config, file);
}

fn set_window_open(app: &tauri::AppHandle, open: bool) {
    if let Some(state) = app.try_state::<ManagedState>() {
        state.controller.set_window_open(open);
    }
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        set_window_open(app, true);
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let Some(config) = app.config().app.windows.first().cloned() else {
        log::warn!("缺少主窗口配置，无法显示窗口");
        return;
    };
    match tauri::WebviewWindowBuilder::from_config(app, &config) {
        Ok(builder) => match builder.build() {
            Ok(window) => {
                let app_handle = app.clone();
                window.on_window_event(move |event| {
                    if matches!(event, tauri::WindowEvent::Destroyed) {
                        set_window_open(&app_handle, false);
                    }
                });
                set_window_open(app, true);
                let _ = window.show();
                let _ = window.set_focus();
            }
            Err(error) => log::warn!("创建主窗口失败: {error}"),
        },
        Err(error) => log::warn!("读取主窗口配置失败: {error}"),
    }
}

#[tauri::command]
fn get_snapshot(state: tauri::State<'_, ManagedState>) -> Snapshot {
    state.controller.snapshot()
}

#[tauri::command]
fn set_paused(
    state: tauri::State<'_, ManagedState>,
    paused: bool,
) -> Result<Snapshot, String> {
    state.controller.set_paused(paused)?;
    Ok(state.controller.snapshot())
}

#[tauri::command]
fn lock_now(state: tauri::State<'_, ManagedState>) -> Result<(), String> {
    state.controller.lock_now()
}

#[tauri::command]
fn refresh_presence(state: tauri::State<'_, ManagedState>) -> Snapshot {
    state.controller.refresh_presence();
    state.controller.snapshot()
}

#[tauri::command]
fn list_discovered(
    state: tauri::State<'_, ManagedState>,
    max_age: Option<f64>,
) -> Vec<DiscoveredDevice> {
    state.controller.list_discovered(max_age.unwrap_or(10.0))
}

#[tauri::command]
fn apply_devices(
    state: tauri::State<'_, ManagedState>,
    devices: Vec<DeviceBinding>,
    policy: String,
) -> Result<Snapshot, String> {
    state
        .controller
        .apply_device_bindings(devices, &policy)?;
    Ok(state.controller.snapshot())
}

#[tauri::command]
fn rename_device(
    state: tauri::State<'_, ManagedState>,
    prefix: String,
    address: String,
    custom_name: Option<String>,
) -> Result<Snapshot, String> {
    state
        .controller
        .rename_device(&prefix, &address, custom_name)?;
    Ok(state.controller.snapshot())
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    const ALLOWED_PREFIX: &str = "https://github.com/yc-2018/PhoneKey";
    if !url.starts_with(ALLOWED_PREFIX) {
        return Err("不支持的链接".to_string());
    }
    let target: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(target.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err("无法打开浏览器".to_string());
    }
    Ok(())
}

#[tauri::command]
fn set_policy(
    state: tauri::State<'_, ManagedState>,
    policy: String,
) -> Result<Snapshot, String> {
    state.controller.set_device_policy(&policy)?;
    Ok(state.controller.snapshot())
}

#[tauri::command]
fn clear_devices(state: tauri::State<'_, ManagedState>) -> Snapshot {
    state.controller.rebind_phone();
    state.controller.snapshot()
}

#[tauri::command]
fn save_password(
    state: tauri::State<'_, ManagedState>,
    password: String,
) -> Result<Snapshot, String> {
    state.controller.save_password(&password)?;
    Ok(state.controller.snapshot())
}

#[tauri::command]
fn clear_password(state: tauri::State<'_, ManagedState>) -> Result<Snapshot, String> {
    state.controller.clear_password()?;
    Ok(state.controller.snapshot())
}

#[tauri::command]
fn update_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, ManagedState>,
    settings: Settings,
) -> Result<Snapshot, String> {
    state.controller.update_settings(settings)?;
    let snapshot = state.controller.snapshot();
    apply_tray_visibility(&app, &snapshot.settings.hide_mode);
    Ok(snapshot)
}

#[tauri::command]
fn set_autostart(
    state: tauri::State<'_, ManagedState>,
    enabled: bool,
) -> Result<Snapshot, String> {
    state.controller.set_autostart(enabled)?;
    Ok(state.controller.snapshot())
}

#[tauri::command]
fn install_cp(
    state: tauri::State<'_, ManagedState>,
) -> Result<Snapshot, String> {
    cpsetup::install_cp(state.resource_dir.as_deref())?;
    state.controller.invalidate_cp_cache();
    Ok(state.controller.snapshot())
}

#[tauri::command]
fn uninstall_cp(
    state: tauri::State<'_, ManagedState>,
) -> Result<Snapshot, String> {
    cpsetup::uninstall_cp(state.resource_dir.as_deref())?;
    state.controller.invalidate_cp_cache();
    Ok(state.controller.snapshot())
}

#[tauri::command]
fn hide_window(app: tauri::AppHandle, state: tauri::State<'_, ManagedState>) {
    state.controller.set_window_open(false);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.destroy();
    }
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle, state: tauri::State<'_, ManagedState>) {
    state.controller.stop();
    app.exit(0);
}

pub fn run() {
    init_logging();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, _args, _cwd| show_main(app),
        ))
        .setup(|app| {
            paths::migrate_legacy_data();
            autostart::migrate_legacy();

            let autostart_enabled = autostart::is_enabled();
            if autostart_enabled {
                if let Err(error) = autostart::set_enabled(true) {
                    log::warn!("刷新开机启动路径失败: {error}");
                }
            }
            let mut loaded = config::load_settings();
            loaded.autostart = autostart_enabled;
            let settings = Arc::new(RwLock::new(loaded));

            let presence = PresenceMonitor::new(settings.clone());
            tauri::async_runtime::spawn(ble::scan_loop(presence.clone()));
            tauri::async_runtime::spawn(wifi::scan_loop(presence.clone()));

            let controller = Controller::new(settings.clone(), presence);
            controller.start();
            app.manage(ManagedState {
                controller: controller.clone(),
                resource_dir: app.path().resource_dir().ok(),
            });

            let paused = controller
                .settings_handle()
                .read()
                .map(|value| value.paused)
                .unwrap_or(false);
            let toggle_label = if paused { "恢复监控" } else { "暂停监控" };
            let show_item = MenuItem::with_id(app, "show", "显示设置", true, None::<&str>)?;
            let toggle_item =
                MenuItem::with_id(app, "toggle_pause", toggle_label, true, None::<&str>)?;
            let lock_item = MenuItem::with_id(app, "lock", "立即锁屏", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&show_item, &toggle_item, &lock_item, &quit_item],
            )?;

            let menu_controller = controller.clone();
            let menu_toggle = toggle_item.clone();
            let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))?;
            TrayIconBuilder::with_id(TRAY_ID)
                .icon(icon)
                .tooltip(format!("{} 手机感应解锁", paths::APP_TITLE))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "show" => show_main(app),
                    "toggle_pause" => {
                        let paused = menu_controller
                            .settings_handle()
                            .read()
                            .map(|value| value.paused)
                            .unwrap_or(false);
                        let _ = menu_controller.set_paused(!paused);
                        let _ = menu_toggle
                            .set_text(if paused { "暂停监控" } else { "恢复监控" });
                    }
                    "lock" => {
                        let _ = menu_controller.lock_now();
                    }
                    "quit" => {
                        if confirm_quit() {
                            menu_controller.stop();
                            app.exit(0);
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;

            let hide_mode = controller
                .settings_handle()
                .read()
                .map(|value| value.hide_mode.clone())
                .unwrap_or_else(|_| "hidden".to_string());
            apply_tray_visibility(app.handle(), &hide_mode);

            let start_hidden = std::env::args().any(|arg| arg == "--autostart");
            if !start_hidden {
                show_main(app.handle());
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            set_paused,
            lock_now,
            refresh_presence,
            list_discovered,
            apply_devices,
            rename_device,
            set_policy,
            clear_devices,
            open_url,
            save_password,
            clear_password,
            update_settings,
            set_autostart,
            install_cp,
            uninstall_cp,
            hide_window,
            quit_app
        ])
        .build(tauri::generate_context!())
        .expect("启动 PhoneKey 失败")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { code, api, .. } if code.is_none() => {
                api.prevent_exit();
            }
            tauri::RunEvent::Exit => {
                if let Some(state) = app.try_state::<ManagedState>() {
                    state.controller.stop();
                }
            }
            _ => {}
        });
}

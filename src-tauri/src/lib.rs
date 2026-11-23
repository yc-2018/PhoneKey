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

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

use config::{DeviceBinding, Settings};
use controller::{Controller, Snapshot};
use presence::{DiscoveredDevice, PresenceMonitor};

struct ManagedState {
    controller: Arc<Controller>,
    resource_dir: Option<PathBuf>,
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

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
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
    state: tauri::State<'_, ManagedState>,
    settings: Settings,
) -> Result<Snapshot, String> {
    state.controller.update_settings(settings)?;
    Ok(state.controller.snapshot())
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
fn hide_window(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
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
            TrayIconBuilder::new()
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
                        menu_controller.stop();
                        app.exit(0);
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

            if let Some(window) = app.get_webview_window("main") {
                let window_for_close = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_for_close.hide();
                    }
                });
            }

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
            set_policy,
            clear_devices,
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
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<ManagedState>() {
                    state.controller.stop();
                }
            }
        });
}

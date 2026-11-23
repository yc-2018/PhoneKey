use std::sync::Arc;
use std::time::{Duration, Instant};

use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::Manager;
use futures_util::StreamExt;
use log::{info, warn};

use crate::presence::{Advertisement, PresenceMonitor};

pub async fn scan_loop(presence: Arc<PresenceMonitor>) {
    let mut last_error = String::new();
    while !presence.is_stopped() {
        let generation = presence.generation();
        let result = scan_once(presence.clone(), generation).await;
        if presence.is_stopped() {
            break;
        }
        if presence.generation() != generation {
            continue;
        }
        let message = match result {
            Ok(()) => "蓝牙扫描已停止".to_string(),
            Err(error) => error,
        };
        if message != last_error {
            warn!("BLE 扫描不可用：{message}");
            last_error = message.clone();
        }
        presence.set_scanning(false, Some(message));
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    presence.set_scanning(false, None);
    info!("BLE 扫描线程已结束");
}

async fn scan_once(
    presence: Arc<PresenceMonitor>,
    generation: u64,
) -> Result<(), String> {
    let manager = Manager::new().await.map_err(|error| format!("初始化蓝牙失败：{error}"))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|error| format!("读取蓝牙适配器失败：{error}"))?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or_else(|| "未找到蓝牙适配器，请确认蓝牙已开启".to_string())?;

    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|error| format!("启动蓝牙扫描失败：{error}"))?;
    presence.set_scanning(true, None);
    info!("BLE 扫描已启动");

    let mut events = adapter
        .events()
        .await
        .map_err(|error| format!("订阅蓝牙事件失败：{error}"))?;
    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_refresh = Instant::now() - Duration::from_secs(5);

    loop {
        if presence.is_stopped() || presence.generation() != generation {
            let _ = adapter.stop_scan().await;
            presence.set_scanning(false, None);
            return Ok(());
        }
        tokio::select! {
            event = events.next() => {
                if let Some(event) = event {
                    if matches!(
                        event,
                        CentralEvent::DeviceDiscovered(_)
                            | CentralEvent::DeviceUpdated(_)
                    ) {
                        if let Some(id) = event_id(&event) {
                            read_peripheral(&adapter, &presence, &id).await;
                        }
                    }
                }
            }
            _ = ticker.tick() => {
                if let Some(binding) = presence.tick() {
                    presence.bind_auto(binding);
                }
                if last_refresh.elapsed() >= Duration::from_secs(2) {
                    last_refresh = Instant::now();
                    refresh_known(&adapter, &presence).await;
                }
            }
        }
    }
}

fn event_id(event: &CentralEvent) -> Option<btleplug::platform::PeripheralId> {
    match event {
        CentralEvent::DeviceDiscovered(id)
        | CentralEvent::DeviceUpdated(id) => Some(id.clone()),
        _ => None,
    }
}

async fn read_peripheral(
    adapter: &btleplug::platform::Adapter,
    presence: &PresenceMonitor,
    id: &btleplug::platform::PeripheralId,
) {
    let peripheral = match adapter.peripheral(id).await {
        Ok(peripheral) => peripheral,
        Err(_) => return,
    };
    let properties = match peripheral.properties().await {
        Ok(Some(properties)) => properties,
        _ => return,
    };
    import_properties(presence, &properties);
}

async fn refresh_known(
    adapter: &btleplug::platform::Adapter,
    presence: &PresenceMonitor,
) {
    let peripherals = match adapter.peripherals().await {
        Ok(peripherals) => peripherals,
        Err(_) => return,
    };
    for peripheral in peripherals {
        if let Ok(Some(properties)) = peripheral.properties().await {
            import_properties(presence, &properties);
        }
    }
}

fn import_properties(
    presence: &PresenceMonitor,
    properties: &btleplug::api::PeripheralProperties,
) {
    let Some(rssi) = properties.rssi else {
        return;
    };
    if properties.address.to_string().is_empty() {
        return;
    }
    let advertisement = Advertisement {
        address: properties.address.to_string().to_ascii_uppercase(),
        name: properties.local_name.clone().unwrap_or_default(),
        rssi,
        service_uuids: properties.services.clone(),
        service_data: properties
            .service_data
            .iter()
            .map(|(uuid, value)| (*uuid, value.clone()))
            .collect(),
        manufacturer_data: properties.manufacturer_data.values().cloned().collect(),
    };
    presence.handle_advertisement(advertisement);
}

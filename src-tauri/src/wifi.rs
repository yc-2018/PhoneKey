use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use log::{info, warn};

use crate::config::DeviceBinding;
use crate::presence::PresenceMonitor;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const PROBE_INTERVAL: Duration = Duration::from_secs(2);

pub async fn scan_loop(presence: Arc<PresenceMonitor>) {
    let mut last_error = String::new();
    while !presence.is_stopped() {
        if !presence.probe_allowed() {
            presence.set_wifi_scanning(false, None);
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }
        let targets = presence.wifi_targets();
        if targets.is_empty() {
            presence.set_wifi_scanning(false, None);
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }

        let Some(ping) = ping_path() else {
            let message = "找不到系统 ping.exe，Wi-Fi 辅助不可用".to_string();
            if message != last_error {
                warn!("{message}");
                last_error = message.clone();
            }
            presence.set_wifi_scanning(false, Some(message));
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        };

        if !last_error.is_empty() {
            last_error.clear();
        }
        presence.set_wifi_scanning(true, None);
        probe_targets(&presence, &ping, targets).await;
        if let Some(binding) = presence.tick() {
            presence.bind_auto(binding);
        }
        tokio::time::sleep(PROBE_INTERVAL).await;
    }
    presence.set_wifi_scanning(false, None);
    info!("Wi-Fi 辅助探测线程已结束");
}

async fn probe_targets(
    presence: &Arc<PresenceMonitor>,
    ping: &PathBuf,
    targets: Vec<DeviceBinding>,
) {
    let mut tasks = Vec::with_capacity(targets.len());
    for binding in targets {
        let ping = ping.clone();
        tasks.push(tokio::task::spawn_blocking(move || {
            let online = ping_host(&ping, &binding.address);
            (binding, online)
        }));
    }
    for task in tasks {
        match task.await {
            Ok((binding, online)) => presence.update_wifi(&binding, online),
            Err(error) => warn!("Wi-Fi 探测任务失败：{error}"),
        }
    }
}

fn ping_path() -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot")?;
    let path = PathBuf::from(root).join("System32").join("ping.exe");
    path.is_file().then_some(path)
}

fn ping_host(ping: &PathBuf, host: &str) -> bool {
    let output = Command::new(ping)
        .args(["-n", "1", "-w", "800", "-4", host])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match output {
        Ok(output) => output.status.success() && has_echo_reply(&output.stdout),
        Err(_) => false,
    }
}

fn has_echo_reply(stdout: &[u8]) -> bool {
    stdout
        .windows(4)
        .any(|window| window.eq_ignore_ascii_case(b"TTL="))
}

pub fn probe_now(presence: &PresenceMonitor) {
    let targets = presence.wifi_targets();
    if targets.is_empty() {
        return;
    }
    let Some(ping) = ping_path() else {
        for binding in &targets {
            presence.update_wifi(binding, false);
        }
        return;
    };
    let results: Vec<(DeviceBinding, bool)> = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(targets.len());
        for binding in targets {
            let ping = ping.clone();
            handles.push(scope.spawn(move || {
                let online = ping_host(&ping, &binding.address);
                (binding, online)
            }));
        }
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect()
    });
    for (binding, online) in results {
        presence.update_wifi(&binding, online);
    }
}

#[cfg(test)]
mod tests {
    use super::has_echo_reply;

    #[test]
    fn echo_reply_requires_ttl_marker() {
        assert!(has_echo_reply(
            b"Reply from 192.168.1.5: bytes=32 time<1ms TTL=64\r\n"
        ));
        assert!(has_echo_reply(b"ttl=64"));
        assert!(!has_echo_reply(b"Request timed out.\r\n"));
        assert!(!has_echo_reply(
            "来自 192.168.1.5 的回复: 无法访问目标主机\r\n".as_bytes()
        ));
    }
}

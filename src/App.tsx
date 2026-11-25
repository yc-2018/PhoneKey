import { useEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import {
  Bluetooth,
  ChevronRight,
  CircleAlert,
  CircleCheck,
  Info,
  KeyRound,
  Loader2,
  Lock,
  Pause,
  Pencil,
  Play,
  Plus,
  Power,
  RefreshCw,
  Radio,
  Save,
  ShieldAlert,
  ShieldCheck,
  SlidersHorizontal,
  Smartphone,
  Sparkles,
  Trash2,
  Wifi,
  X,
} from "lucide-react";
import { api } from "./api";
import type {
  DeviceBinding,
  DevicePolicy,
  DeviceView,
  PresenceState,
  Settings,
  Snapshot,
  TabKey,
} from "./types";
import { DevicePicker } from "./components/DevicePicker";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { PasswordDialog } from "./components/PasswordDialog";
import { RenameDialog } from "./components/RenameDialog";
import { formatRssi, RssiBars } from "./components/RssiBars";
import { Toggle } from "./components/Toggle";

const APP_VERSION = "1.2.3";
const RELEASES_URL = "https://github.com/yc-2018/PhoneKey/releases/latest";
const FEEDBACK_URL = "https://github.com/yc-2018/PhoneKey/issues/new";

type ConfirmRequest = {
  title: string;
  message: ReactNode;
  note?: ReactNode;
  noteTone?: "info" | "warning";
  confirmLabel: string;
  tone?: "accent" | "warning" | "danger";
  icon: ReactNode;
  onConfirm: () => void;
};

function bindingKey(prefix: string, address: string): string {
  if (prefix) return `p:${prefix}`;
  if (address) return `a:${address}`;
  return "";
}

function formatAge(age: number | null): string {
  if (age == null) return "尚未检测到";
  if (age < 1.5) return "刚刚更新";
  if (age < 60) return `${Math.round(age)} 秒前`;
  return `${Math.floor(age / 60)} 分钟前`;
}

function stateText(
  state: PresenceState,
  scanning: boolean,
  paused: boolean,
  wifiOnly: boolean,
): string {
  if (paused) return "已暂停";
  if (!scanning) return "扫描未启动";
  if (state === "near") return wifiOnly ? "在线" : "在附近";
  if (state === "away") return wifiOnly ? "离线" : "已离开";
  return "等待检测";
}

function stateClass(state: PresenceState, scanning: boolean): string {
  if (!scanning) return "unknown";
  return state;
}

function SliderRow({
  label,
  hint,
  value,
  min,
  max,
  step = 1,
  unit,
  onChange,
  format,
}: {
  label: string;
  hint: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  unit: string;
  onChange: (value: number) => void;
  format?: (value: number) => string;
}) {
  const percent = ((value - min) / (max - min)) * 100;
  return (
    <div className="slider-row">
      <div className="slider-head">
        <div>
          <span className="slider-label">{label}</span>
          <span className="slider-hint">{hint}</span>
        </div>
        <span className="slider-value">
          {format ? format(value) : value}
          {unit && <em>{unit}</em>}
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        style={{ "--range-progress": `${percent}%` } as CSSProperties}
        onChange={(event) => onChange(Number(event.target.value))}
        onWheel={(event) => {
          if (document.activeElement === event.currentTarget) {
            event.currentTarget.blur();
          }
        }}
      />
    </div>
  );
}

function SettingSwitch({
  title,
  description,
  checked,
  onChange,
  disabled,
}: {
  title: string;
  description: string;
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <div className={`switch-row ${disabled ? "disabled" : ""}`}>
      <div>
        <span className="switch-title">{title}</span>
        <span className="switch-description">{description}</span>
      </div>
      <Toggle
        checked={checked}
        onChange={onChange}
        disabled={disabled}
        label={title}
      />
    </div>
  );
}

function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [tab, setTab] = useState<TabKey>("devices");
  const [pickerOpen, setPickerOpen] = useState(false);
  const [renameTarget, setRenameTarget] = useState<DeviceView | null>(null);
  const [passwordOpen, setPasswordOpen] = useState(false);
  const [confirmRequest, setConfirmRequest] = useState<ConfirmRequest | null>(null);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [connectionError, setConnectionError] = useState("");
  const [notice, setNotice] = useState("");

  const settingsRef = useRef<Settings | null>(null);
  const dirtyRef = useRef(false);
  const saveTimer = useRef<number | null>(null);
  const saveSeq = useRef(0);
  const noticeTimer = useRef<number | null>(null);

  useEffect(() => {
    let alive = true;
    const refresh = async () => {
      try {
        const next = await api.snapshot();
        if (!alive) return;
        setSnapshot(next);
        setConnectionError("");
        if (!dirtyRef.current) {
          settingsRef.current = next.settings;
          setSettings(next.settings);
        }
      } catch (err) {
        if (alive) setConnectionError(String(err));
      }
    };
    refresh();
    const timer = window.setInterval(refresh, 700);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    return () => {
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
      if (noticeTimer.current) window.clearTimeout(noticeTimer.current);
    };
  }, []);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: light)");
    const apply = () => {
      const mode = settingsRef.current?.theme ?? "system";
      const light = mode === "light" || (mode === "system" && media.matches);
      document.documentElement.dataset.theme = light ? "light" : "dark";
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [settings?.theme]);

  const flash = (message: string) => {
    setNotice(message);
    if (noticeTimer.current) window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(""), 3600);
  };

  const openLink = (url: string) => {
    void api.openUrl(url).catch((err) => setError(String(err)));
  };

  const applyServerSnapshot = (next: Snapshot, force = true) => {
    setSnapshot(next);
    if (force || !dirtyRef.current) {
      dirtyRef.current = false;
      settingsRef.current = next.settings;
      setSettings(next.settings);
    }
  };

  const perform = async (
    label: string,
    action: () => Promise<Snapshot | void>,
    successMessage?: string,
  ) => {
    setBusy(label);
    setError("");
    try {
      const result = await action();
      if (result) applyServerSnapshot(result, true);
      if (successMessage) flash(successMessage);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy("");
    }
  };

  const flushSettings = async (
    sequence: number,
    next: Settings,
  ): Promise<boolean> => {
    try {
      const result = await api.updateSettings(next);
      if (sequence !== saveSeq.current) return false;
      dirtyRef.current = false;
      applyServerSnapshot(result, true);
      return true;
    } catch (err) {
      if (sequence !== saveSeq.current) return false;
      dirtyRef.current = false;
      setError(String(err));
      const fresh = await api.snapshot().catch(() => null);
      if (fresh) applyServerSnapshot(fresh, true);
      return false;
    }
  };

  const queueSettings = (next: Settings, delay: number) => {
    settingsRef.current = next;
    setSettings(next);
    dirtyRef.current = true;
    if (saveTimer.current) window.clearTimeout(saveTimer.current);
    const sequence = ++saveSeq.current;
    saveTimer.current = window.setTimeout(() => {
      saveTimer.current = null;
      void flushSettings(sequence, next);
    }, delay);
  };

  const hideApp = async () => {
    if (saveTimer.current) {
      window.clearTimeout(saveTimer.current);
      saveTimer.current = null;
    }
    if (dirtyRef.current && settingsRef.current) {
      const saved = await flushSettings(++saveSeq.current, settingsRef.current);
      if (!saved) return;
    }
    try {
      await api.hideWindow();
    } catch {
      // 窗口销毁时 IPC 可能提前断开
    }
  };

  const updateSetting = <K extends keyof Settings>(
    key: K,
    value: Settings[K],
    delay = 260,
  ) => {
    const current = settingsRef.current;
    if (!current) return;
    const next = { ...current, [key]: value };
    if (key === "near_rssi" && next.far_rssi > (next.near_rssi as number) - 4) {
      next.far_rssi = Math.max(-110, (next.near_rssi as number) - 8);
    }
    queueSettings(next, delay);
  };

  const account = snapshot
    ? snapshot.domain && snapshot.domain !== "."
      ? `${snapshot.domain}\\${snapshot.account}`
      : snapshot.account
    : "";

  if (!snapshot || !settings) {
    return (
      <div className="app-shell boot-screen">
        <div className="boot-mark">
          <KeyRound size={30} />
        </div>
        <h1>PhoneKey</h1>
        <p>{connectionError || "正在连接本机服务…"}</p>
        {connectionError && (
          <button className="secondary-button" onClick={() => location.reload()}>
            重新连接
          </button>
        )}
      </div>
    );
  }

  const { presence } = snapshot;
  const paused = settings.paused;
  const scanning = presence.scanning && !presence.error;
  const wifiOnly =
    presence.devices.length > 0 &&
    presence.devices.every((device) => device.kind === "wifi");
  const primaryDevice =
    presence.devices.find((device) => device.present) ?? presence.devices[0];
  const statusLabel = stateText(presence.state, scanning, paused, wifiOnly);
  const statusTone = stateClass(presence.state, scanning);
  const primaryName =
    presence.phone_name ||
    presence.devices[0]?.name ||
    (presence.phone_bound ? "已绑定设备" : "等待识别设备");
  const activeDevices = presence.devices.filter((device) => device.present);
  const sourceNote =
    presence.source === "wifi"
      ? "Wi-Fi 辅助"
      : presence.source === "mixed"
        ? "含 Wi-Fi 辅助"
        : "";
  const visibleError = error || connectionError;

  const togglePause = () => {
    perform(
      paused ? "恢复监控" : "暂停监控",
      () => api.setPaused(!paused),
      paused ? "已恢复监控" : "已暂停自动锁屏与手机解锁",
    );
  };

  const lockNow = () => {
    setConfirmRequest({
      title: "立即锁定此电脑？",
      message: "锁屏后需要点击「手机感应解锁」并检测手机。",
      note: "请先保存未完成的工作。",
      noteTone: "warning",
      confirmLabel: "立即锁定",
      tone: "warning",
      icon: <Lock size={19} />,
      onConfirm: () => perform("锁定电脑", () => api.lockNow(), "已发送锁屏请求"),
    });
  };

  const applyDevices = async (devices: DeviceBinding[], policy: DevicePolicy) => {
    const result = await api.applyDevices(devices, policy);
    applyServerSnapshot(result, true);
    flash(`已更新解锁设备，共 ${result.settings.bound_devices.length} 台`);
  };

  const removeDevice = (prefix: string, address: string) => {
    const current = settingsRef.current;
    if (!current) return;
    const key = bindingKey(prefix, address);
    const remaining = current.bound_devices.filter(
      (device) => bindingKey(device.prefix, device.address) !== key,
    );
    perform(
      "移除设备",
      () => api.applyDevices(remaining, current.device_policy),
      "已移除设备",
    );
  };

  const setPolicy = (policy: DevicePolicy) => {
    if (policy === settings.device_policy) return;
    perform(
      "更新解锁策略",
      () => api.setPolicy(policy),
      policy === "all" ? "已设为全部设备都在附近时解锁" : "已设为任一设备在附近时解锁",
    );
  };

  const saveAutostart = (enabled: boolean) => {
    perform(
      enabled ? "开启开机自启" : "关闭开机自启",
      () => api.setAutostart(enabled),
      enabled ? "已开启开机自启" : "已关闭开机自启",
    );
  };

  const clearDevices = () => {
    setConfirmRequest({
      title: "清除所有设备绑定？",
      message: "程序会重新扫描并自动识别手机系统广播。",
      confirmLabel: "重新识别手机",
      tone: "danger",
      icon: <RefreshCw size={19} />,
      onConfirm: () =>
        perform("重新识别手机", () => api.clearDevices(), "已清除绑定，正在重新识别手机"),
    });
  };

  const refreshPresence = () => {
    perform("刷新检测", () => api.refreshPresence(), "已重新启动蓝牙扫描");
  };

  const installCp = () => {
    setConfirmRequest({
      title: "安装或更新 Windows 解锁组件？",
      message:
        "这会请求管理员权限，将 UnlockProvider.dll 写入 System32，并在注册表中注册锁屏凭证提供程序。",
      note: "请确认当前没有正在进行的重要工作。",
      noteTone: "warning",
      confirmLabel: "安装组件",
      tone: "danger",
      icon: <ShieldAlert size={19} />,
      onConfirm: () =>
        perform("安装解锁组件", () => api.installCp(), "解锁组件已安装"),
    });
  };

  const uninstallCp = () => {
    setConfirmRequest({
      title: "卸载 Windows 解锁组件？",
      message: "手机感应解锁选项将从锁屏界面移除，PIN 和密码登录不受影响。",
      confirmLabel: "卸载组件",
      tone: "danger",
      icon: <Trash2 size={19} />,
      onConfirm: () =>
        perform("卸载解锁组件", () => api.uninstallCp(), "解锁组件已卸载"),
    });
  };

  const clearPassword = () => {
    setConfirmRequest({
      title: "清除已保存的 Windows 密码？",
      message: "手机解锁选项将不可用，直到重新保存密码。",
      confirmLabel: "清除密码",
      tone: "danger",
      icon: <KeyRound size={19} />,
      onConfirm: () =>
        perform("清除密码", () => api.clearPassword(), "已清除保存的密码"),
    });
  };

  const quitApp = () => {
    setConfirmRequest({
      title: "退出 PhoneKey？",
      message: "退出后将停止手机检测，锁屏界面不能再使用手机感应解锁。",
      note: "重新启动 PhoneKey 后可恢复；PIN 或密码仍可正常登录。",
      confirmLabel: "退出软件",
      tone: "danger",
      icon: <Power size={19} />,
      onConfirm: () => perform("退出软件", () => api.quit()),
    });
  };

  const renameBinding =
    renameTarget && settings
      ? settings.bound_devices.find(
          (device) =>
            bindingKey(device.prefix, device.address) ===
            bindingKey(renameTarget.prefix, renameTarget.address),
        )
      : undefined;

  return (
    <div className="app-shell">
      <header className="titlebar" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          <div className="brand-mark">
            <KeyRound size={20} />
          </div>
          <div className="brand-copy" data-tauri-drag-region>
            <h1>PhoneKey</h1>
            <p>手机感应解锁</p>
          </div>
        </div>
        <div className="titlebar-actions">
          <div
            className={`monitor-pill ${paused ? "paused" : ""} ${
              !scanning && !paused ? "starting" : ""
            }`}
          >
            <span className="pulse-dot" />
            {paused ? "已暂停" : scanning ? "监控中" : "准备中"}
          </div>
          <button
            className="window-button close"
            aria-label={settings.hide_mode === "hidden" ? "完全隐藏窗口" : "关闭到托盘"}
            onClick={() => void hideApp()}
          >
            <X size={16} />
          </button>
        </div>
      </header>

      <main className="content">
        <section className={`status-card tone-${statusTone}`}>
          <div className="status-glow" />
          <div className="status-main">
            <div className="status-copy">
              <span className="eyebrow">
                <Sparkles size={12} /> 当前状态
              </span>
              <div className="device-headline">
                <h2>{primaryName}</h2>
                <span className={`state-pill tone-${statusTone}`}>
                  <span className="state-dot" />
                  {statusLabel}
                </span>
              </div>
              <p className="status-meta">
                {presence.phone_bound
                  ? `${presence.device_count} 台设备 · ${
                      settings.device_policy === "all" ? "全部可用" : "任一可用"
                    }${sourceNote ? ` · ${sourceNote}` : ""}`
                  : "等待绑定解锁设备"}
                {presence.error ? ` · ${presence.error}` : ""}
              </p>
              <div className="status-details">
                <span>
                  {primaryDevice?.kind === "wifi" ? (
                    <>
                      <Wifi size={13} />
                      {primaryDevice.present ? "Wi-Fi 在线" : "Wi-Fi 未响应"}
                    </>
                  ) : (
                    <>
                      <Radio size={13} />
                      {formatRssi(presence.smoothed ?? presence.rssi)}
                    </>
                  )}
                </span>
                <span>
                  <RefreshCw size={12} />
                  {formatAge(presence.last_seen_age)}
                </span>
                {snapshot.locked && snapshot.lock_check && (
                  <span className={`lock-check ${snapshot.lock_check}`}>
                    <Lock size={11} />
                    锁屏校验：{snapshot.lock_check === "near" ? "通过" : "未通过"}
                  </span>
                )}
                {snapshot.remote && (
                  <span className="lock-check">
                    <ShieldAlert size={11} />
                    远程会话
                  </span>
                )}
              </div>
            </div>
            <div className={`signal-visual ${statusTone}`}>
              <div className="signal-ring">
                {presence.state === "near" && !paused && (
                  <span className="signal-ripple" />
                )}
                {wifiOnly ? (
                  <Wifi size={30} strokeWidth={1.6} />
                ) : (
                  <RssiBars rssi={presence.smoothed ?? presence.rssi} large />
                )}
              </div>
              <span className="signal-caption">
                {activeDevices.length > 0
                  ? `${activeDevices.length} 台${wifiOnly ? "在线" : "在附近"}`
                  : "等待信号"}
              </span>
            </div>
          </div>
          <div className="status-actions">
            <button className="action-button" onClick={togglePause} disabled={!!busy}>
              {paused ? <Play size={15} /> : <Pause size={15} />}
              {paused ? "开始监控" : "停止监控"}
            </button>
            <button className="action-button primary" onClick={lockNow} disabled={!!busy}>
              <Lock size={15} />
              锁屏
            </button>
          </div>
        </section>

        {visibleError && (
          <div className="banner error">
            <CircleAlert size={15} />
            <span>{visibleError}</span>
            <button
              className="banner-close"
              onClick={() => {
                setError("");
                setConnectionError("");
              }}
            >
              <X size={14} />
            </button>
          </div>
        )}
        {notice && !visibleError && (
          <div className="banner success">
            <CircleCheck size={15} />
            <span>{notice}</span>
          </div>
        )}

        <nav className="tabs">
          {(
            [
              ["devices", "设备", <Smartphone size={15} />],
              ["unlock", "解锁", <KeyRound size={15} />],
              ["rules", "规则", <SlidersHorizontal size={15} />],
            ] as const
          ).map(([key, label, icon]) => (
            <button
              key={key}
              className={tab === key ? "active" : ""}
              onClick={() => setTab(key)}
            >
              {icon}
              {label}
              {key === "devices" && (
                <span className="tab-count">{presence.device_count}</span>
              )}
            </button>
          ))}
        </nav>

        <div className="tab-panel">
          {tab === "devices" && (
            <section className="tab-section">
              <div className="section-heading">
                <div>
                  <h3>解锁设备</h3>
                  <p>小米手机广播或 Wi-Fi 辅助设备在线时，锁屏界面才允许点击解锁</p>
                </div>
                <button className="add-button" onClick={() => setPickerOpen(true)}>
                  <Plus size={15} />
                  添加设备
                </button>
              </div>

              <div className="policy-card">
                <div>
                  <span className="policy-title">解锁条件</span>
                  <span className="policy-description">
                    {settings.device_policy === "all"
                      ? "所有绑定设备都可用时，才允许点击解锁"
                      : "任意一台绑定设备可用时，即可点击解锁"}
                  </span>
                </div>
                <div className="segmented">
                  <button
                    className={settings.device_policy === "any" ? "active" : ""}
                    onClick={() => setPolicy("any")}
                    disabled={!!busy}
                  >
                    任一设备
                  </button>
                  <button
                    className={settings.device_policy === "all" ? "active" : ""}
                    onClick={() => setPolicy("all")}
                    disabled={!!busy}
                  >
                    全部设备
                  </button>
                </div>
              </div>

              <div className="device-list">
                {presence.devices.length === 0 && (
                  <div className="empty-card">
                    <div className="empty-icon">
                      <Bluetooth size={22} />
                    </div>
                    <h4>尚未绑定设备</h4>
                    <p>
                      点击「添加设备」选择小米手机广播或 BLE 设备。
                      其他品牌手机只能添加 Wi-Fi 辅助，且只能判断同一局域网。
                    </p>
                    <button className="text-button" onClick={() => setPickerOpen(true)}>
                      搜索附近设备 <ChevronRight size={14} />
                    </button>
                  </div>
                )}

                {presence.devices.map((device) => (
                  <article
                    className={`device-card ${device.present ? "present" : ""}`}
                    key={bindingKey(device.prefix, device.address) || device.address}
                  >
                    <div
                      className={`device-icon ${
                        device.kind === "wifi" ? "wifi" : device.kind === "phone" ? "phone" : ""
                      }`}
                    >
                      {device.kind === "phone" ? (
                        <Smartphone size={18} />
                      ) : device.kind === "wifi" ? (
                        <Wifi size={18} />
                      ) : (
                        <Bluetooth size={18} />
                      )}
                    </div>
                    <div className="device-card-body">
                      <div className="device-card-title">
                        <strong>{device.name || "未命名设备"}</strong>
                        <span className={`kind-tag ${device.kind}`}>
                          {device.kind === "phone"
                            ? "手机广播"
                            : device.kind === "wifi"
                              ? "Wi-Fi 辅助"
                              : "BLE"}
                        </span>
                      </div>
                      <span className="device-address">
                        {device.address || "等待设备地址"}
                      </span>
                      <div className="device-card-status">
                        <span
                          className={`device-state ${
                            device.present ? "near" : "away"
                          }`}
                        >
                          <span />
                          {device.kind === "wifi"
                            ? device.present
                              ? "局域网在线"
                              : "未响应"
                            : device.present
                              ? "在附近"
                              : formatAge(device.last_seen_age)}
                        </span>
                        <span className={`device-signal ${device.kind === "wifi" ? "wifi" : ""}`}>
                          {device.kind === "wifi" ? (
                            <>
                              <Wifi size={11} />
                              同一局域网
                            </>
                          ) : (
                            <>
                              <RssiBars rssi={device.smoothed ?? device.rssi} />
                              {formatRssi(device.smoothed ?? device.rssi)}
                            </>
                          )}
                        </span>
                      </div>
                    </div>
                    <div className="device-card-actions">
                      <button
                        className="icon-button"
                        aria-label="重命名设备"
                        title="重命名设备"
                        onClick={() => setRenameTarget(device)}
                        disabled={!!busy}
                      >
                        <Pencil size={15} />
                      </button>
                      <button
                        className="icon-button danger"
                        aria-label="移除设备"
                        onClick={() => removeDevice(device.prefix, device.address)}
                        disabled={!!busy}
                      >
                        <Trash2 size={15} />
                      </button>
                    </div>
                  </article>
                ))}
              </div>

              {presence.devices.some((device) => device.kind === "wifi") && (
                <div className="wifi-hint">
                  <CircleAlert size={14} />
                  <span>
                    Wi-Fi 辅助只判断是否在同一局域网，不判断距离；手机休眠、路由器隔离或
                    IP 变化都可能误判，仍必须点击锁屏磁贴才会解锁。
                  </span>
                </div>
              )}

              <button
                className="wide-ghost-button"
                onClick={() => setPickerOpen(true)}
              >
                <Plus size={15} />
                添加或更换解锁设备
              </button>
            </section>
          )}

          {tab === "unlock" && (
            <section className="tab-section">
              <div className="section-heading">
                <div>
                  <h3>锁屏解锁</h3>
                  <p>手机靠近不会自动解锁，必须由你点击锁屏界面上的解锁按钮</p>
                </div>
              </div>

              <div className="settings-card">
                <SettingSwitch
                  title="点击解锁"
                  description="锁屏时提供「手机感应解锁」，点击后实时检测手机"
                  checked={settings.click_unlock}
                  onChange={(value) => updateSetting("click_unlock", value, 0)}
                />
                <div className="card-divider" />
                <div className="status-line">
                  <span className={`status-line-icon ${snapshot.cp_installed ? "ok" : "warn"}`}>
                    {snapshot.cp_installed ? (
                      <ShieldCheck size={16} />
                    ) : (
                      <ShieldAlert size={16} />
                    )}
                  </span>
                  <div>
                    <strong>
                      {snapshot.cp_installed ? "解锁组件已就绪" : "解锁组件未安装"}
                    </strong>
                    <span>
                      {snapshot.cp_installed
                        ? "锁屏界面会显示手机解锁选项"
                        : "安装后才能从锁屏界面使用手机解锁"}
                    </span>
                  </div>
                </div>
              </div>

              <div className="settings-card">
                <div className="card-title-row">
                  <div>
                    <h4>Windows 密码</h4>
                    <p>使用账户密码，不是 PIN，也不是 Windows Hello</p>
                  </div>
                  <span className={`soft-badge ${snapshot.has_password ? "ok" : "warn"}`}>
                    {snapshot.has_password ? "已保存" : "未保存"}
                  </span>
                </div>
                <div className="account-chip">
                  <KeyRound size={14} />
                  {account || "当前 Windows 账户"}
                </div>
                <div className="button-row">
                  <button
                    className="secondary-button"
                    onClick={() => setPasswordOpen(true)}
                  >
                    <Save size={14} />
                    {snapshot.has_password ? "更换密码" : "保存密码"}
                  </button>
                  {snapshot.has_password && (
                    <button
                      className="ghost-button danger-text"
                      onClick={clearPassword}
                      disabled={!!busy}
                    >
                      清除密码
                    </button>
                  )}
                </div>
              </div>

              <div className="settings-card">
                <div className="card-title-row">
                  <div>
                    <h4>Windows 登录组件</h4>
                    <p>写入 System32 并注册到锁屏界面，需要管理员权限</p>
                  </div>
                </div>
                <div className="button-row">
                  <button
                    className="secondary-button"
                    onClick={installCp}
                    disabled={!!busy}
                  >
                    {busy === "安装解锁组件" ? (
                      <Loader2 size={14} className="spin" />
                    ) : (
                      <ShieldCheck size={14} />
                    )}
                    {snapshot.cp_installed ? "重新安装 / 更新" : "安装解锁组件"}
                  </button>
                  {snapshot.cp_installed && (
                    <button
                      className="ghost-button danger-text"
                      onClick={uninstallCp}
                      disabled={!!busy}
                    >
                      卸载组件
                    </button>
                  )}
                </div>
                <div className="info-note">
                  <Info size={14} />
                  <span>
                    组件只在本机锁屏界面提交你保存的密码。每次点击都会重新检测手机，
                    手机不在附近时不会提交。
                  </span>
                </div>
              </div>

              <div className="tip-card">
                <Lock size={16} />
                <p>
                  仍需保留 PIN 或密码作为备用登录方式。手机没电、蓝牙关闭或组件停止运行时，
                  可以使用系统原本的登录方式。
                </p>
              </div>
            </section>
          )}

          {tab === "rules" && (
            <section className="tab-section">
              <div className="settings-card">
                <div className="card-title-row">
                  <div>
                    <h4>外观主题</h4>
                    <p>默认跟随 Windows 系统主题，也可固定深色或浅色</p>
                  </div>
                </div>
                <div className="segmented window-mode">
                  <button
                    className={settings.theme === "system" ? "active" : ""}
                    onClick={() => updateSetting("theme", "system", 0)}
                    disabled={!!busy}
                  >
                    跟随系统
                  </button>
                  <button
                    className={settings.theme === "dark" ? "active" : ""}
                    onClick={() => updateSetting("theme", "dark", 0)}
                    disabled={!!busy}
                  >
                    深色
                  </button>
                  <button
                    className={settings.theme === "light" ? "active" : ""}
                    onClick={() => updateSetting("theme", "light", 0)}
                    disabled={!!busy}
                  >
                    浅色
                  </button>
                </div>
              </div>

              <div className="section-heading">
                <div>
                  <h3>自动化规则</h3>
                  <p>调整信号阈值和锁屏判定，修改会自动保存</p>
                </div>
              </div>

              <div className="settings-card">
                <SettingSwitch
                  title="离开后自动锁屏"
                  description="钥匙设备从附近变为离开后，锁定此电脑"
                  checked={settings.auto_lock}
                  onChange={(value) => updateSetting("auto_lock", value, 0)}
                />
                <div className="card-divider" />
                <SettingSwitch
                  title="开机自动启动"
                  description="登录 Windows 后在后台运行，并隐藏主窗口"
                  checked={settings.autostart}
                  onChange={saveAutostart}
                />
              </div>

              <div className="settings-card">
                <div className="card-title-row">
                  <div>
                    <h4>信号阈值</h4>
                    <p>数值越接近 0 信号越强；修改后立即生效</p>
                  </div>
                </div>
                <SliderRow
                  label="判定在附近"
                  hint="平滑信号高于此值时视为靠近"
                  value={settings.near_rssi}
                  min={-100}
                  max={-40}
                  unit="dBm"
                  onChange={(value) => updateSetting("near_rssi", value)}
                />
                <SliderRow
                  label="判定已远离"
                  hint="持续低于此值时开始累计离开时间"
                  value={settings.far_rssi}
                  min={-110}
                  max={Math.max(-50, settings.near_rssi - 4)}
                  unit="dBm"
                  onChange={(value) => updateSetting("far_rssi", value)}
                />
                <div className="threshold-visual">
                  <span>强</span>
                  <div className="threshold-track">
                    <span
                      className="threshold-zone near"
                      style={{
                        left: `${((-settings.near_rssi - 40) / 70) * 100}%`,
                      }}
                    />
                    <span
                      className="threshold-zone far"
                      style={{
                        left: `${((-settings.far_rssi - 40) / 70) * 100}%`,
                      }}
                    />
                  </div>
                  <span>弱</span>
                </div>
              </div>

              <div className="settings-card">
                <div className="card-title-row">
                  <div>
                    <h4>判定时间</h4>
                    <p>避免信号抖动造成误锁屏或误判</p>
                  </div>
                </div>
                <SliderRow
                  label="靠近确认"
                  hint="信号达到近场阈值后，持续多久才算在附近"
                  value={settings.near_confirm}
                  min={0.5}
                  max={10}
                  step={0.5}
                  unit="秒"
                  onChange={(value) => updateSetting("near_confirm", value)}
                  format={(value) => value.toFixed(1)}
                />
                <SliderRow
                  label="离开判定"
                  hint="信号持续偏低多久后判定为离开"
                  value={settings.away_timeout}
                  min={5}
                  max={120}
                  unit="秒"
                  onChange={(value) => updateSetting("away_timeout", value)}
                />
                <SliderRow
                  label="锁屏缓冲"
                  hint="判定离开后，等待多久再执行锁屏"
                  value={settings.lock_grace}
                  min={0}
                  max={60}
                  step={0.5}
                  unit="秒"
                  onChange={(value) => updateSetting("lock_grace", value)}
                  format={(value) => value.toFixed(1)}
                />
                <SliderRow
                  label="解锁选项延迟"
                  hint="进入锁屏后，等待多久向锁屏界面提供手机解锁"
                  value={settings.unlock_delay}
                  min={0}
                  max={15}
                  step={0.5}
                  unit="秒"
                  onChange={(value) => updateSetting("unlock_delay", value)}
                  format={(value) => value.toFixed(1)}
                />
                {settings.auto_lock && (
                  <div className="info-note">
                    <Info size={14} />
                    <span>
                      手机广播模式下，离开判定至少为 30 秒，避免锁定过早。
                      当前实际判定时间约为{" "}
                      {Math.max(settings.away_timeout, 30)} 秒。
                    </span>
                  </div>
                )}
              </div>

              <div className="settings-card">
                <div className="card-title-row">
                  <div>
                    <h4>扫描与维护</h4>
                    <p>蓝牙扫描、设备绑定和远程会话状态</p>
                  </div>
                  <span className={`soft-badge ${scanning ? "ok" : "warn"}`}>
                    {scanning ? "扫描中" : "已停止"}
                  </span>
                </div>
                <div className="button-row">
                  <button
                    className="secondary-button"
                    onClick={refreshPresence}
                    disabled={!!busy}
                  >
                    <RefreshCw size={14} className={busy === "刷新检测" ? "spin" : ""} />
                    刷新设备检测
                  </button>
                  <button
                    className="ghost-button"
                    onClick={clearDevices}
                    disabled={!!busy}
                  >
                    重新识别手机
                  </button>
                </div>
                <div className="stat-grid">
                  <div>
                    <span>绑定设备</span>
                    <strong>{presence.device_count} / 8</strong>
                  </div>
                  <div>
                    <span>当前信号</span>
                    <strong>{formatRssi(presence.smoothed ?? presence.rssi)}</strong>
                  </div>
                  <div>
                    <span>锁屏状态</span>
                    <strong>{snapshot.locked ? "已锁定" : "已解锁"}</strong>
                  </div>
                  <div>
                    <span>远程会话</span>
                    <strong>{snapshot.remote ? "是" : "否"}</strong>
                  </div>
                </div>
              </div>

              <div className="settings-card">
                <div className="card-title-row">
                  <div>
                    <h4>窗口隐藏</h4>
                    <p>关闭或最小化窗口后的隐藏方式，后台解锁仍会继续</p>
                  </div>
                </div>
                <div className="segmented window-mode">
                  <button
                    className={settings.hide_mode === "hidden" ? "active" : ""}
                    onClick={() => updateSetting("hide_mode", "hidden", 0)}
                    disabled={!!busy}
                  >
                    完全隐藏
                  </button>
                  <button
                    className={settings.hide_mode === "tray" ? "active" : ""}
                    onClick={() => updateSetting("hide_mode", "tray", 0)}
                    disabled={!!busy}
                  >
                    隐藏到托盘
                  </button>
                </div>
                <div className="info-note">
                  <Info size={14} />
                  <span>
                    {settings.hide_mode === "hidden"
                      ? "关闭后完全隐藏，重新点击 PhoneKey 快捷方式可再次打开设置。"
                      : "关闭后隐藏到托盘，点击托盘图标可再次打开设置。"}
                  </span>
                </div>
              </div>

              <button
                className="wide-quit-button"
                onClick={quitApp}
                disabled={!!busy}
              >
                <Power size={15} />
                退出软件
              </button>
            </section>
          )}
        </div>
      </main>

      <footer className="statusbar">
        <div className={`footer-state ${snapshot.last_error ? "error" : ""}`}>
          <span className={`footer-dot ${scanning && !paused ? "live" : ""}`} />
          <span title={snapshot.last_action}>
            {snapshot.last_error || snapshot.last_action || "已就绪"}
          </span>
        </div>
        <div className="footer-links">
          <button className="footer-link" onClick={() => openLink(RELEASES_URL)}>
            检查更新
          </button>
          <button className="footer-link" onClick={() => openLink(FEEDBACK_URL)}>
            意见反馈
          </button>
          <span className="version">v{APP_VERSION}</span>
        </div>
      </footer>

      {busy && (
        <div className="busy-toast">
          <Loader2 size={14} className="spin" />
          {busy}…
        </div>
      )}

      {pickerOpen && (
        <DevicePicker
          current={settings.bound_devices}
          policy={settings.device_policy}
          onClose={() => setPickerOpen(false)}
          onApply={applyDevices}
        />
      )}

      {renameTarget && (
        <RenameDialog
          device={renameTarget}
          currentName={renameBinding?.custom_name ?? ""}
          onClose={() => setRenameTarget(null)}
          onSaved={(next, message) => {
            applyServerSnapshot(next, true);
            setRenameTarget(null);
            flash(message);
          }}
        />
      )}

      {passwordOpen && (
        <PasswordDialog
          hasPassword={snapshot.has_password}
          account={account}
          onClose={() => setPasswordOpen(false)}
          onSaved={(next) => {
            applyServerSnapshot(next, true);
            flash("Windows 密码已加密保存");
          }}
        />
      )}

      {confirmRequest && (
        <ConfirmDialog
          title={confirmRequest.title}
          message={confirmRequest.message}
          note={confirmRequest.note}
          noteTone={confirmRequest.noteTone}
          confirmLabel={confirmRequest.confirmLabel}
          tone={confirmRequest.tone}
          icon={confirmRequest.icon}
          onCancel={() => setConfirmRequest(null)}
          onConfirm={() => {
            const run = confirmRequest.onConfirm;
            setConfirmRequest(null);
            run();
          }}
        />
      )}
    </div>
  );
}

export default App;

import { useEffect, useMemo, useState } from "react";
import {
  Bluetooth,
  Check,
  CircleAlert,
  Loader2,
  Plus,
  Radio,
  Search,
  Smartphone,
  Wifi,
  X,
} from "lucide-react";
import { api } from "../api";
import type {
  DeviceBinding,
  DevicePolicy,
  DiscoveredDevice,
} from "../types";
import { formatRssi, RssiBars } from "./RssiBars";

type Filter = "all" | "phone" | "ble";

function bindingKey(device: Pick<DeviceBinding, "prefix" | "address">): string {
  if (device.prefix) return `p:${device.prefix}`;
  if (device.address) return `a:${device.address}`;
  return "";
}

function normalizeWifiHost(value: string): string {
  const host = value.trim().toLowerCase();
  if (
    !host ||
    host.length > 253 ||
    host.startsWith("-") ||
    host.endsWith("-") ||
    !/^[a-z0-9.-]+$/.test(host)
  ) {
    return "";
  }
  if (/^[0-9.]+$/.test(host)) {
    const parts = host.split(".");
    if (parts.length !== 4) return "";
    for (const part of parts) {
      if (
        !/^\d{1,3}$/.test(part) ||
        (part.length > 1 && part.startsWith("0")) ||
        Number(part) > 255
      ) {
        return "";
      }
    }
    return host;
  }
  if (
    host
      .split(".")
      .some(
        (label) =>
          !label ||
          label.length > 63 ||
          label.startsWith("-") ||
          label.endsWith("-"),
      )
  ) {
    return "";
  }
  return host;
}

export function DevicePicker({
  current,
  policy,
  onClose,
  onApply,
}: {
  current: DeviceBinding[];
  policy: DevicePolicy;
  onClose: () => void;
  onApply: (devices: DeviceBinding[], policy: DevicePolicy) => Promise<void>;
}) {
  const [devices, setDevices] = useState<DiscoveredDevice[]>([]);
  const [selected, setSelected] = useState<Map<string, DeviceBinding>>(
    () =>
      new Map(
        current
          .map((device) => [bindingKey(device), device] as const)
          .filter(([key]) => Boolean(key)),
      ),
  );
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [showManual, setShowManual] = useState(false);
  const [manualAddress, setManualAddress] = useState("");
  const [manualName, setManualName] = useState("");
  const [showWifi, setShowWifi] = useState(false);
  const [wifiHost, setWifiHost] = useState("");
  const [wifiName, setWifiName] = useState("");

  useEffect(() => {
    let alive = true;
    const load = async () => {
      try {
        const items = await api.listDiscovered(12);
        if (alive) {
          setDevices(items);
          setError("");
        }
      } catch (err) {
        if (alive) setError(String(err));
      } finally {
        if (alive) setLoading(false);
      }
    };
    load();
    const timer = window.setInterval(load, 1200);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const counts = useMemo(() => {
    const phone = devices.filter((device) => device.kind === "phone").length;
    return { all: devices.length, phone, ble: devices.length - phone };
  }, [devices]);

  const wifiBindings = useMemo(
    () =>
      Array.from(selected.values()).filter((device) => device.kind === "wifi"),
    [selected],
  );

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return devices.filter((device) => {
      if (filter === "phone" && device.kind !== "phone") return false;
      if (filter === "ble" && device.kind === "phone") return false;
      if (!needle) return true;
      return (
        device.name.toLowerCase().includes(needle) ||
        device.address.toLowerCase().includes(needle) ||
        device.prefix.toLowerCase().includes(needle)
      );
    });
  }, [devices, filter, query]);

  const toggle = (device: DiscoveredDevice) => {
    const binding: DeviceBinding = {
      name: device.name,
      address: device.address,
      prefix: device.kind === "phone" ? device.prefix : "",
      kind: device.kind === "phone" ? "phone" : "ble",
    };
    const key = bindingKey(binding);
    setSelected((previous) => {
      const next = new Map(previous);
      if (next.has(key)) {
        next.delete(key);
      } else {
        if (next.size >= 8) {
          setError("最多绑定 8 台设备");
          return previous;
        }
        next.set(key, binding);
      }
      return next;
    });
    setError("");
  };

  const addManual = () => {
    const normalized = manualAddress.trim().toUpperCase().replace(/-/g, ":");
    if (!/^([0-9A-F]{2}:){5}[0-9A-F]{2}$/.test(normalized)) {
      setError("请输入正确的 MAC 地址，例如 AA:BB:CC:DD:EE:FF");
      return;
    }
    if (selected.size >= 8) {
      setError("最多绑定 8 台设备");
      return;
    }
    const binding: DeviceBinding = {
      name: manualName.trim().slice(0, 40),
      address: normalized,
      prefix: "",
      kind: "ble",
    };
    setSelected((previous) => {
      const next = new Map(previous);
      next.set(bindingKey(binding), binding);
      return next;
    });
    setManualAddress("");
    setManualName("");
    setShowManual(false);
    setError("");
  };

  const addWifi = () => {
    const host = normalizeWifiHost(wifiHost);
    if (!host) {
      setError("请输入正确的手机 IP 或主机名，例如 192.168.1.23");
      return;
    }
    const binding: DeviceBinding = {
      name: wifiName.trim().slice(0, 40) || "Wi-Fi 设备",
      address: host,
      prefix: "",
      kind: "wifi",
    };
    const key = bindingKey(binding);
    if (selected.has(key)) {
      setError("该 Wi-Fi 设备已添加");
      return;
    }
    if (selected.size >= 8) {
      setError("最多绑定 8 台设备");
      return;
    }
    setSelected((previous) => {
      const next = new Map(previous);
      next.set(key, binding);
      return next;
    });
    setWifiHost("");
    setWifiName("");
    setShowWifi(false);
    setError("");
  };

  const removeSelected = (key: string) => {
    setSelected((previous) => {
      const next = new Map(previous);
      next.delete(key);
      return next;
    });
    setError("");
  };

  const apply = async () => {
    setSaving(true);
    setError("");
    try {
      await onApply(Array.from(selected.values()), policy);
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div
        className="modal device-picker"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="modal-head">
          <div>
            <h2>添加解锁设备</h2>
            <p>选择小米手机广播、BLE 设备，或添加 Wi-Fi 辅助设备</p>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭">
            <X size={18} />
          </button>
        </header>

        <div className="picker-notice">
          <CircleAlert size={16} />
          <div>
            <strong>手机广播目前仅支持小米</strong>
            <span>
              其他品牌手机不会出现在“小米手机”列表；普通 BLE 设备仍可按固定 MAC
              绑定。没有蓝牙广播时，可添加 Wi-Fi 辅助，但它只能判断同一局域网，不能判断距离。
            </span>
          </div>
        </div>

        <div className="search-box">
          <Search size={16} />
          <input
            autoFocus
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="搜索名称或 MAC 地址"
          />
          {loading && <Loader2 size={15} className="spin" />}
        </div>

        <div className="picker-tabs">
          <div className="segmented compact">
            {(
              [
                ["all", "全部", counts.all],
                ["phone", "小米手机", counts.phone],
                ["ble", "BLE", counts.ble],
              ] as const
            ).map(([key, label, count]) => (
              <button
                key={key}
                className={filter === key ? "active" : ""}
                onClick={() => setFilter(key)}
              >
                {label}
                <span>{count}</span>
              </button>
            ))}
          </div>
          <div className="picker-tools">
            <button
              className={`text-button ${showWifi ? "active" : ""}`}
              onClick={() => {
                setShowWifi((value) => !value);
                setShowManual(false);
              }}
            >
              <Wifi size={14} /> Wi-Fi 辅助
            </button>
            <button
              className={`text-button ${showManual ? "active" : ""}`}
              onClick={() => {
                setShowManual((value) => !value);
                setShowWifi(false);
              }}
            >
              <Plus size={14} /> 手动绑定
            </button>
          </div>
        </div>

        {showWifi && (
          <div className="wifi-box">
            <div className="wifi-warning">
              <CircleAlert size={15} />
              <span>
                Wi-Fi 只能判断手机和电脑是否在同一局域网，不能判断距离。手机休眠或禁止
                ping、路由器 AP 隔离、IP 变化、访客设备都可能误判，同一网络中的设备也可能冒充。
                它只作辅助，仍必须点击锁屏磁贴才会解锁。
              </span>
            </div>
            <div className="wifi-fields">
              <input
                value={wifiName}
                onChange={(event) => setWifiName(event.target.value)}
                placeholder="设备名称（可选）"
                maxLength={40}
              />
              <input
                value={wifiHost}
                onChange={(event) => setWifiHost(event.target.value)}
                placeholder="手机 IP 或主机名，如 192.168.1.23"
                spellCheck={false}
              />
              <button className="small-button" onClick={addWifi}>
                添加
              </button>
            </div>
          </div>
        )}

        {showManual && (
          <div className="manual-row">
            <input
              value={manualName}
              onChange={(event) => setManualName(event.target.value)}
              placeholder="设备名称（可选）"
              maxLength={40}
            />
            <input
              value={manualAddress}
              onChange={(event) => setManualAddress(event.target.value)}
              placeholder="AA:BB:CC:DD:EE:FF"
              spellCheck={false}
            />
            <button className="small-button" onClick={addManual}>
              添加
            </button>
          </div>
        )}

        <div className="picker-list">
          {visible.length === 0 && wifiBindings.length === 0 && !loading && (
            <div className="picker-empty">
              <Radio size={22} />
              <p>{query ? "没有匹配的设备" : "附近暂未扫描到设备"}</p>
              <span>
                请确认蓝牙已开启，手机广播处于可发现状态；其他品牌手机可改用 Wi-Fi 辅助。
              </span>
            </div>
          )}
          {wifiBindings.length > 0 && (
            <div className="picker-section-label">已添加的 Wi-Fi 辅助</div>
          )}
          {wifiBindings.map((binding) => (
            <button
              key={bindingKey(binding)}
              className="picker-row selected"
              title="点击移除"
              onClick={() => removeSelected(bindingKey(binding))}
            >
              <span className="check checked">
                <Check size={12} strokeWidth={3} />
              </span>
              <span className="device-icon wifi">
                <Wifi size={17} />
              </span>
              <span className="picker-device-info">
                <span className="picker-device-name">
                  {binding.name || "Wi-Fi 设备"}
                </span>
                <span className="picker-device-meta">
                  {binding.address}
                  <em>Wi-Fi 辅助</em>
                </span>
              </span>
              <span className="picker-wifi-hint">同一局域网</span>
            </button>
          ))}
          {visible.map((device) => {
            const key =
              device.kind === "phone" && device.prefix
                ? `p:${device.prefix}`
                : `a:${device.address}`;
            const checked = selected.has(key);
            return (
              <button
                key={`${device.address}-${device.prefix}`}
                className={`picker-row ${checked ? "selected" : ""}`}
                onClick={() => toggle(device)}
              >
                <span className={`check ${checked ? "checked" : ""}`}>
                  {checked && <Check size={12} strokeWidth={3} />}
                </span>
                <span className={`device-icon ${device.kind === "phone" ? "phone" : ""}`}>
                  {device.kind === "phone" ? (
                    <Smartphone size={17} />
                  ) : (
                    <Bluetooth size={17} />
                  )}
                </span>
                <span className="picker-device-info">
                  <span className="picker-device-name">
                    {device.name || "未命名设备"}
                  </span>
                  <span className="picker-device-meta">
                    {device.address}
                    {device.kind === "phone" && <em>手机广播</em>}
                  </span>
                </span>
                <span className="picker-rssi">
                  <RssiBars rssi={device.rssi} />
                  <span>{formatRssi(device.rssi)}</span>
                </span>
              </button>
            );
          })}
        </div>

        {error && <div className="inline-error">{error}</div>}

        <footer className="modal-actions">
          <span className="selection-count">已选择 {selected.size} / 8</span>
          <div>
            <button className="secondary-button" onClick={onClose}>
              取消
            </button>
            <button
              className="primary-button"
              onClick={apply}
              disabled={saving}
            >
              {saving && <Loader2 size={15} className="spin" />}
              确定选择
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
}

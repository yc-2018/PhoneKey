import { useEffect, useMemo, useState } from "react";
import {
  Bluetooth,
  Check,
  Loader2,
  Plus,
  Radio,
  Search,
  Smartphone,
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
            <p>选择手机广播，或按 MAC 地址绑定普通 BLE 设备</p>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭">
            <X size={18} />
          </button>
        </header>

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
                ["phone", "手机", counts.phone],
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
          <button
            className="text-button"
            onClick={() => setShowManual((value) => !value)}
          >
            <Plus size={14} /> 手动绑定
          </button>
        </div>

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
          {visible.length === 0 && !loading && (
            <div className="picker-empty">
              <Radio size={22} />
              <p>{query ? "没有匹配的设备" : "附近暂未扫描到设备"}</p>
              <span>请确认蓝牙已开启，手机广播处于可发现状态</span>
            </div>
          )}
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

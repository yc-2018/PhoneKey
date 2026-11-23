import { useState } from "react";
import { Loader2, Pencil, RotateCcw, X } from "lucide-react";
import { api } from "../api";
import type { DeviceView, Snapshot } from "../types";

export function RenameDialog({
  device,
  currentName,
  onClose,
  onSaved,
}: {
  device: DeviceView;
  currentName: string;
  onClose: () => void;
  onSaved: (snapshot: Snapshot, message: string) => void;
}) {
  const original = device.original_name || device.name;
  const [name, setName] = useState(currentName || original);
  const [saving, setSaving] = useState("");
  const [error, setError] = useState("");

  const persist = async (customName: string | null, message: string) => {
    setSaving(customName ? "save" : "reset");
    setError("");
    try {
      const snapshot = await api.renameDevice(
        device.prefix,
        device.address,
        customName,
      );
      onSaved(snapshot, message);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving("");
    }
  };

  const save = () => {
    const trimmed = name.trim();
    if (!trimmed) {
      setError("请输入设备名称");
      return;
    }
    const customName = trimmed === original ? null : trimmed;
    void persist(
      customName,
      customName ? "已重命名设备" : "已恢复原来的设备名称",
    );
  };

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div
        className="modal rename-dialog"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="modal-head">
          <div className="modal-title-row">
            <span className="modal-icon">
              <Pencil size={18} />
            </span>
            <div>
              <h2>重命名设备</h2>
              <p>原名：{original || "未命名设备"}</p>
            </div>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭">
            <X size={18} />
          </button>
        </header>

        <label className="field-label" htmlFor="device-name">
          设备名称
        </label>
        <div className="password-field">
          <input
            id="device-name"
            autoFocus
            value={name}
            maxLength={40}
            autoComplete="off"
            onChange={(event) => {
              setName(event.target.value);
              setError("");
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") save();
            }}
            placeholder="输入设备名称"
          />
        </div>

        {error && <div className="inline-error">{error}</div>}

        <footer className="modal-actions">
          <button
            className="text-button"
            onClick={() => void persist(null, "已恢复原来的设备名称")}
            disabled={!currentName || !!saving}
            title={currentName ? "恢复设备本身的名字" : "当前使用的是原名"}
          >
            <RotateCcw size={13} />
            重置为原名
          </button>
          <div>
            <button className="secondary-button" onClick={onClose}>
              取消
            </button>
            <button className="primary-button" onClick={save} disabled={!!saving}>
              {saving === "save" && <Loader2 size={15} className="spin" />}
              保存
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
}

import { useState } from "react";
import { Eye, EyeOff, KeyRound, Loader2, ShieldCheck, X } from "lucide-react";
import { api } from "../api";
import type { Snapshot } from "../types";

export function PasswordDialog({
  hasPassword,
  account,
  onClose,
  onSaved,
}: {
  hasPassword: boolean;
  account: string;
  onClose: () => void;
  onSaved: (snapshot: Snapshot) => void;
}) {
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [visible, setVisible] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const save = async () => {
    if (!password) {
      setError("请输入 Windows 账户密码");
      return;
    }
    if (password.length > 200) {
      setError("密码过长");
      return;
    }
    if (password !== confirm) {
      setError("两次输入的密码不一致");
      return;
    }
    setSaving(true);
    setError("");
    try {
      const snapshot = await api.savePassword(password);
      setPassword("");
      setConfirm("");
      onSaved(snapshot);
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
        className="modal password-dialog"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="modal-head">
          <div className="modal-title-row">
            <span className="modal-icon">
              <KeyRound size={19} />
            </span>
            <div>
              <h2>{hasPassword ? "修改 Windows 密码" : "保存 Windows 密码"}</h2>
              <p>账户：{account}</p>
            </div>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭">
            <X size={18} />
          </button>
        </header>

        <div className="password-note">
          <ShieldCheck size={16} />
          <p>
            请填写 Windows 账户密码，不是 PIN，也不是 Windows Hello。
            密码使用系统 DPAPI 加密保存在本机，仅当你在锁屏界面点击手机解锁且手机在附近时提交。
          </p>
        </div>

        <label className="field-label" htmlFor="password">
          Windows 密码
        </label>
        <div className="password-field">
          <input
            id="password"
            autoFocus
            type={visible ? "text" : "password"}
            value={password}
            maxLength={200}
            autoComplete="off"
            onChange={(event) => setPassword(event.target.value)}
            placeholder="输入账户密码"
          />
          <button
            className="field-action"
            onClick={() => setVisible((value) => !value)}
            aria-label={visible ? "隐藏密码" : "显示密码"}
            type="button"
          >
            {visible ? <EyeOff size={16} /> : <Eye size={16} />}
          </button>
        </div>

        <label className="field-label" htmlFor="password-confirm">
          确认密码
        </label>
        <div className="password-field">
          <input
            id="password-confirm"
            type={visible ? "text" : "password"}
            value={confirm}
            maxLength={200}
            autoComplete="off"
            onChange={(event) => setConfirm(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") save();
            }}
            placeholder="再次输入密码"
          />
        </div>

        {error && <div className="inline-error">{error}</div>}

        <footer className="modal-actions">
          <span className="muted-hint">密码不会上传，也不会写入日志</span>
          <div>
            <button className="secondary-button" onClick={onClose}>
              取消
            </button>
            <button className="primary-button" onClick={save} disabled={saving}>
              {saving && <Loader2 size={15} className="spin" />}
              保存密码
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
}

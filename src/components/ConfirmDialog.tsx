import { useEffect } from "react";
import type { ReactNode } from "react";

export function ConfirmDialog({
  title,
  message,
  note,
  noteTone = "info",
  confirmLabel,
  tone = "accent",
  icon,
  onConfirm,
  onCancel,
}: {
  title: string;
  message: ReactNode;
  note?: ReactNode;
  noteTone?: "info" | "warning";
  confirmLabel: string;
  tone?: "accent" | "warning" | "danger";
  icon: ReactNode;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  return (
    <div className="modal-backdrop" onMouseDown={onCancel}>
      <div
        className={`modal confirm-dialog tone-${tone}`}
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="modal-head">
          <div className="modal-title-row">
            <span className="modal-icon">{icon}</span>
            <div>
              <h2>{title}</h2>
            </div>
          </div>
        </header>

        <div className="confirm-body">
          <p>{message}</p>
          {note && (
            <div className={`confirm-note ${noteTone === "warning" ? "warning" : ""}`}>
              {note}
            </div>
          )}
        </div>

        <footer className="modal-actions confirm-actions">
          <div>
            <button className="secondary-button" onClick={onCancel} autoFocus>
              取消
            </button>
            <button className="primary-button" onClick={onConfirm}>
              {confirmLabel}
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
}

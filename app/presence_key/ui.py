from __future__ import annotations

import logging
import os
import queue
import threading
import tkinter as tk
from tkinter import messagebox

import customtkinter as ctk
from PIL import Image

from . import __version__, autostart, credential, cpsetup, secret
from .config import DEFAULT_SETTINGS, save_settings
from .paths import APP_TITLE, log_path, resource
from .presence import STATE_AWAY, STATE_NEAR

log = logging.getLogger(__name__)

BG = ("#F3F5FA", "#0E1421")
CARD = ("#FFFFFF", "#161E2C")
TEXT = ("#111827", "#E8ECF4")
MUTED = ("#6B7280", "#93A0B4")
LINE = ("#E5E7EB", "#243044")
ACCENT = "#3B82F6"
ACCENT_HOVER = "#2563EB"
GREEN = "#16A34A"
AMBER = "#D97706"
RED = "#DC2626"


class MainWindow(ctk.CTk):
    def __init__(self, controller, on_hide):
        super().__init__(fg_color=BG)
        self.controller = controller
        self._on_hide = on_hide
        self._refreshing = False

        self.title(APP_TITLE)
        width, height = 620, 760
        x = max(0, (self.winfo_screenwidth() - width) // 2)
        y = max(0, (self.winfo_screenheight() - height - 80) // 2)
        self.geometry(f"{width}x{height}+{x}+{y}")
        self.minsize(560, 620)
        try:
            self.iconbitmap(str(resource("resources", "icon.ico")))
        except Exception:
            pass
        self.protocol("WM_DELETE_WINDOW", self._hide)

        self._font_title = ctk.CTkFont(family="Microsoft YaHei UI", size=22, weight="bold")
        self._font_section = ctk.CTkFont(family="Microsoft YaHei UI", size=12, weight="bold")
        self._font_body = ctk.CTkFont(family="Microsoft YaHei UI", size=14)
        self._font_body_bold = ctk.CTkFont(family="Microsoft YaHei UI", size=14, weight="bold")
        self._font_small = ctk.CTkFont(family="Microsoft YaHei UI", size=12)
        self._font_status = ctk.CTkFont(family="Microsoft YaHei UI", size=26, weight="bold")

        self._icon_image = ctk.CTkImage(
            Image.open(resource("resources", "icon.png")), size=(46, 46)
        )

        self._auto_lock_var = tk.BooleanVar(value=self.controller.settings.auto_lock)
        self._click_unlock_var = tk.BooleanVar(value=self.controller.settings.click_unlock)
        self._pause_var = tk.BooleanVar(value=self.controller.settings.paused)
        self._autostart_var = tk.BooleanVar(value=autostart.is_enabled())
        self._setup_hint_visible = False
        self._posts: queue.Queue = queue.Queue()

        self._build()
        self.after(300, self._refresh)

    # ------------------------------------------------------------------
    # Layout
    # ------------------------------------------------------------------

    def _build(self) -> None:
        header = ctk.CTkFrame(self, fg_color="transparent")
        header.pack(fill="x", padx=24, pady=(18, 10))
        ctk.CTkLabel(header, text="", image=self._icon_image).pack(side="left")
        titles = ctk.CTkFrame(header, fg_color="transparent")
        titles.pack(side="left", padx=(14, 0), fill="x", expand=True)
        ctk.CTkLabel(
            titles, text=APP_TITLE, font=self._font_title, text_color=TEXT, anchor="w"
        ).pack(fill="x")
        ctk.CTkLabel(
            titles,
            text="手机在附近时提供锁屏解锁选项，点击后才解锁",
            font=self._font_small,
            text_color=MUTED,
            anchor="w",
        ).pack(fill="x")

        self._tabs = ctk.CTkTabview(
            self,
            fg_color=BG,
            segmented_button_fg_color=("gray90", "gray20"),
            segmented_button_selected_color=ACCENT,
            segmented_button_selected_hover_color=ACCENT_HOVER,
            segmented_button_unselected_color=("gray90", "gray20"),
            segmented_button_unselected_hover_color=("gray85", "gray25"),
            text_color=TEXT,
            segmented_button_font=self._font_small,
            corner_radius=10,
        )
        self._tabs.pack(fill="both", expand=True, padx=18, pady=(0, 2))
        state_tab = self._tabs.add("状态")
        setup_tab = self._tabs.add("解锁设置")

        state_scroll = ctk.CTkScrollableFrame(
            state_tab,
            fg_color=BG,
            corner_radius=0,
            scrollbar_fg_color=BG,
            scrollbar_button_color=("gray75", "gray35"),
            scrollbar_button_hover_color=("gray65", "gray45"),
        )
        state_scroll.pack(fill="both", expand=True)
        setup_scroll = ctk.CTkScrollableFrame(
            setup_tab,
            fg_color=BG,
            corner_radius=0,
            scrollbar_fg_color=BG,
            scrollbar_button_color=("gray75", "gray35"),
            scrollbar_button_hover_color=("gray65", "gray45"),
        )
        setup_scroll.pack(fill="both", expand=True)

        self._build_status(state_scroll)
        self._build_automation(state_scroll)
        self._build_unlock(setup_scroll)
        self._build_startup(setup_scroll)
        self._build_footer()

    def _build_status(self, parent) -> None:
        card = ctk.CTkFrame(parent, fg_color=CARD, corner_radius=16)
        card.pack(fill="x", padx=16, pady=(4, 10))
        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="x", padx=22, pady=18)

        self._status_title = ctk.CTkLabel(
            inner,
            text="正在启动…",
            font=self._font_status,
            text_color=TEXT,
            anchor="w",
        )
        self._status_title.pack(fill="x")
        self._status_detail = ctk.CTkLabel(
            inner,
            text="正在准备蓝牙扫描",
            font=self._font_small,
            text_color=MUTED,
            anchor="w",
            justify="left",
            wraplength=450,
        )
        self._status_detail.pack(fill="x", pady=(4, 0))
        self._status_extra = ctk.CTkLabel(
            inner,
            text="",
            font=self._font_small,
            text_color=MUTED,
            anchor="w",
            justify="left",
            wraplength=450,
        )
        self._status_extra.pack(fill="x", pady=(2, 0))

        row = ctk.CTkFrame(inner, fg_color="transparent")
        row.pack(fill="x", pady=(10, 0))
        ctk.CTkLabel(
            row, text="手机信号", font=self._font_small, text_color=MUTED, anchor="w"
        ).pack(side="left")
        self._signal_value = ctk.CTkLabel(
            row, text="—", font=self._font_body_bold, text_color=TEXT
        )
        self._signal_value.pack(side="right")

        self._signal_note = ctk.CTkLabel(
            inner,
            text="等待手机广播…",
            font=self._font_small,
            text_color=MUTED,
            anchor="w",
            justify="left",
            wraplength=450,
        )
        self._signal_note.pack(fill="x", pady=(2, 0))

        self._setup_hint = ctk.CTkLabel(
            inner,
            text="手机解锁尚未就绪 · 点击前往「解锁设置」 →",
            text_color=AMBER,
            font=self._font_small,
            anchor="w",
            cursor="hand2",
        )
        self._setup_hint.bind("<Button-1>", lambda event: self._tabs.set("解锁设置"))

    def _build_automation(self, parent) -> None:
        inner = self._section(parent, "自动操作")
        self._switch_row(
            inner,
            "自动锁屏",
            "手机离开确认后自动锁定屏幕",
            self._auto_lock_var,
            self._on_auto_lock,
        )
        self._switch_row(
            inner,
            "锁屏点击解锁",
            "手机在附近时显示解锁选项，点击后才提交密码",
            self._click_unlock_var,
            self._on_click_unlock,
        )
        self._divider(inner)
        self._switch_row(
            inner,
            "暂停全部自动操作",
            "暂时只扫描手机，不锁屏也不提供解锁选项",
            self._pause_var,
            self._on_pause,
        )
        row = ctk.CTkFrame(inner, fg_color="transparent")
        row.pack(fill="x", pady=(10, 0))
        ctk.CTkButton(
            row,
            text="立即锁屏",
            width=96,
            height=30,
            corner_radius=8,
            fg_color=("gray85", "gray25"),
            hover_color=("gray75", "gray35"),
            text_color=TEXT,
            font=self._font_small,
            command=self.controller.lock_now,
        ).pack(side="left")
        ctk.CTkButton(
            row,
            text="高级设置",
            width=96,
            height=30,
            corner_radius=8,
            fg_color="transparent",
            border_width=1,
            border_color=LINE,
            hover_color=("gray90", "gray22"),
            text_color=TEXT,
            font=self._font_small,
            command=self._open_advanced,
        ).pack(side="left", padx=(10, 0))
        ctk.CTkLabel(
            inner,
            text="手机靠近只会显示解锁选项，必须点击后才会解锁。",
            font=self._font_small,
            text_color=MUTED,
            anchor="w",
            justify="left",
            wraplength=450,
        ).pack(fill="x", pady=(8, 0))

    def _build_unlock(self, parent) -> None:
        inner = self._section(parent, "锁屏解锁组件")

        row = ctk.CTkFrame(inner, fg_color="transparent")
        row.pack(fill="x", pady=4)
        left = ctk.CTkFrame(row, fg_color="transparent")
        left.pack(side="left", fill="x", expand=True)
        ctk.CTkLabel(
            left,
            text="Windows 解锁组件",
            font=self._font_body_bold,
            text_color=TEXT,
            anchor="w",
        ).pack(fill="x")
        self._cp_note = ctk.CTkLabel(
            left,
            text="安装后锁屏界面才会出现手机解锁选项",
            font=self._font_small,
            text_color=MUTED,
            anchor="w",
            wraplength=330,
            justify="left",
        )
        self._cp_note.pack(fill="x")
        self._cp_status = ctk.CTkLabel(
            row, text="检查中…", font=self._font_body_bold, text_color=MUTED
        )
        self._cp_status.pack(side="right", padx=(12, 0))

        buttons = ctk.CTkFrame(inner, fg_color="transparent")
        buttons.pack(fill="x", pady=(6, 0))
        self._install_btn = ctk.CTkButton(
            buttons,
            text="安装组件",
            width=100,
            height=30,
            corner_radius=8,
            fg_color=ACCENT,
            hover_color=ACCENT_HOVER,
            font=self._font_small,
            command=self._install_cp,
        )
        self._install_btn.pack(side="left")
        self._uninstall_btn = ctk.CTkButton(
            buttons,
            text="卸载组件",
            width=100,
            height=30,
            corner_radius=8,
            fg_color=("gray85", "gray25"),
            hover_color=("gray75", "gray35"),
            text_color=TEXT,
            font=self._font_small,
            command=self._uninstall_cp,
        )
        self._uninstall_btn.pack(side="left", padx=(10, 0))

        self._divider(inner)

        prow = ctk.CTkFrame(inner, fg_color="transparent")
        prow.pack(fill="x", pady=4)
        pleft = ctk.CTkFrame(prow, fg_color="transparent")
        pleft.pack(side="left", fill="x", expand=True)
        ctk.CTkLabel(
            pleft,
            text="Windows 密码",
            font=self._font_body_bold,
            text_color=TEXT,
            anchor="w",
        ).pack(fill="x")
        ctk.CTkLabel(
            pleft,
            text=f"账户：{credential.account_name()}（使用账户密码，不是 PIN）",
            font=self._font_small,
            text_color=MUTED,
            anchor="w",
        ).pack(fill="x")
        self._password_status = ctk.CTkLabel(
            prow, text="未保存", font=self._font_body_bold, text_color=MUTED
        )
        self._password_status.pack(side="right", padx=(12, 0))

        pbuttons = ctk.CTkFrame(inner, fg_color="transparent")
        pbuttons.pack(fill="x", pady=(6, 0))
        self._password_btn = ctk.CTkButton(
            pbuttons,
            text="设置密码",
            width=100,
            height=30,
            corner_radius=8,
            fg_color=ACCENT,
            hover_color=ACCENT_HOVER,
            font=self._font_small,
            command=self._open_password,
        )
        self._password_btn.pack(side="left")
        self._clear_password_btn = ctk.CTkButton(
            pbuttons,
            text="清除密码",
            width=100,
            height=30,
            corner_radius=8,
            fg_color=("gray85", "gray25"),
            hover_color=("gray75", "gray35"),
            text_color=TEXT,
            font=self._font_small,
            command=self._clear_password,
        )
        self._clear_password_btn.pack(side="left", padx=(10, 0))

        ctk.CTkLabel(
            inner,
            text="密码使用 Windows DPAPI 加密保存在本机，不写入配置或日志。安装组件需要管理员权限，会向 System32 写入 DLL 并注册到锁屏界面。",
            font=self._font_small,
            text_color=MUTED,
            anchor="w",
            justify="left",
            wraplength=450,
        ).pack(fill="x", pady=(10, 0))

    def _build_startup(self, parent) -> None:
        inner = self._section(parent, "其他")
        self._switch_row(
            inner,
            "开机自动启动",
            "登录 Windows 后在后台运行",
            self._autostart_var,
            self._on_autostart,
        )

    def _build_footer(self) -> None:
        footer = ctk.CTkFrame(self, fg_color="transparent")
        footer.pack(fill="x", side="bottom", padx=24, pady=(4, 14))
        self._action_label = ctk.CTkLabel(
            footer,
            text="",
            font=self._font_small,
            text_color=MUTED,
            anchor="w",
            wraplength=560,
            justify="left",
        )
        self._action_label.pack(fill="x")
        self._error_label = ctk.CTkLabel(
            footer,
            text="",
            font=self._font_small,
            text_color=RED,
            anchor="w",
            wraplength=560,
            justify="left",
        )
        self._error_label.pack(fill="x")
        row = ctk.CTkFrame(footer, fg_color="transparent")
        row.pack(fill="x", pady=(4, 0))
        ctk.CTkButton(
            row,
            text="打开日志文件夹",
            width=110,
            height=26,
            corner_radius=8,
            fg_color="transparent",
            border_width=1,
            border_color=LINE,
            hover_color=("gray90", "gray22"),
            text_color=MUTED,
            font=self._font_small,
            command=self._open_logs,
        ).pack(side="left")
        ctk.CTkLabel(
            row, text=f"v{__version__}", font=self._font_small, text_color=MUTED
        ).pack(side="right")

    def _section(self, parent, title: str):
        card = ctk.CTkFrame(parent, fg_color=CARD, corner_radius=16)
        card.pack(fill="x", padx=16, pady=(4, 10))
        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="x", padx=22, pady=18)
        ctk.CTkLabel(
            inner,
            text=title,
            font=self._font_section,
            text_color=MUTED,
            anchor="w",
        ).pack(fill="x", pady=(0, 8))
        return inner

    @staticmethod
    def _divider(parent) -> None:
        ctk.CTkFrame(parent, height=1, fg_color=LINE).pack(fill="x", pady=8)

    def _switch_row(self, parent, title, desc, variable, command) -> None:
        row = ctk.CTkFrame(parent, fg_color="transparent")
        row.pack(fill="x", pady=5)
        text = ctk.CTkFrame(row, fg_color="transparent")
        text.pack(side="left", fill="x", expand=True)
        ctk.CTkLabel(
            text, text=title, font=self._font_body_bold, text_color=TEXT, anchor="w"
        ).pack(fill="x")
        ctk.CTkLabel(
            text,
            text=desc,
            font=self._font_small,
            text_color=MUTED,
            anchor="w",
            justify="left",
            wraplength=390,
        ).pack(fill="x")
        ctk.CTkSwitch(
            row,
            text="",
            width=48,
            variable=variable,
            command=command,
            progress_color=ACCENT,
        ).pack(side="right", padx=(12, 0))

    # ------------------------------------------------------------------
    # State refresh
    # ------------------------------------------------------------------

    def post(self, fn, *args) -> None:
        self._posts.put((fn, args))

    def _drain_posts(self) -> None:
        while True:
            try:
                fn, args = self._posts.get_nowait()
            except queue.Empty:
                return
            try:
                fn(*args)
            except Exception:
                log.exception("界面回调失败")

    def _refresh(self) -> None:
        if not self.winfo_exists():
            return
        self._drain_posts()
        if not self._refreshing:
            self._refreshing = True
            try:
                self._apply_snapshot(self.controller.snapshot())
            except Exception:
                log.exception("刷新界面失败")
            finally:
                self._refreshing = False
        self.after(500, self._refresh)

    def refresh_now(self) -> None:
        try:
            self._apply_snapshot(self.controller.snapshot())
        except Exception:
            log.exception("刷新界面失败")

    def _apply_snapshot(self, snap: dict) -> None:
        presence = snap["presence"]
        paused = snap["paused"]
        remote = snap["remote"]
        state = presence["state"]
        source = snap.get("source")
        source_label = "手机广播" if source in ("phone", "app") else ""
        source_prefix = f"{source_label} · " if source_label else ""
        phone_bound = snap.get("phone_bound")
        phone_name = snap.get("phone_name") or "手机"

        if paused:
            self._set_status("已暂停", "自动锁屏与手机解锁已关闭，广播扫描仍在进行", MUTED)
        elif remote:
            self._set_status("远程会话中", "已跳过自动锁屏与手机解锁", MUTED)
        elif not presence["scanning"]:
            error = presence["error"]
            if error:
                self._set_status("蓝牙不可用", f"扫描失败：{error}", RED)
            else:
                self._set_status("正在启动…", "正在准备蓝牙扫描", MUTED)
        elif state == STATE_NEAR:
            rssi = presence["rssi"]
            age = presence["last_seen_age"] or 0
            self._set_status(
                "手机在附近",
                f"{source_prefix}信号 {rssi} dBm · {age:.0f} 秒前收到广播",
                GREEN,
            )
        elif state == STATE_AWAY:
            self._set_status("手机已离开", "已确认离开；重新靠近后可在锁屏点击解锁", AMBER)
        elif presence["seen_since_start"]:
            self._set_status(
                "正在判断距离…", f"{source_prefix}已检测到手机广播，正在确认远近", MUTED
            )
        elif phone_bound:
            self._set_status(
                "等待手机广播",
                f"已绑定「{phone_name}」，请保持手机蓝牙开启",
                MUTED,
            )
        elif snap.get("phone_adv_enabled", True):
            self._set_status(
                "正在识别手机广播",
                "保持手机蓝牙开启，识别到后会自动绑定",
                MUTED,
            )
        else:
            self._set_status("等待手机广播", "请保持手机蓝牙开启", MUTED)

        if paused:
            extra_text = "已暂停"
            extra_color = MUTED
        elif snap["locked"]:
            if not self.controller.settings.click_unlock:
                extra_text = "当前屏幕：已锁定 · 手机解锁已关闭"
                extra_color = MUTED
            elif not (snap["cp_installed"] and snap["has_password"]):
                extra_text = "当前屏幕：已锁定 · 手机解锁尚未就绪"
                extra_color = AMBER
            elif snap["unlock_offered"]:
                extra_text = "当前屏幕：已锁定 · 解锁选项已就绪，请点击锁屏上的解锁按钮"
                extra_color = GREEN
            elif state == STATE_NEAR:
                extra_text = "当前屏幕：已锁定 · 正在准备解锁选项…"
                extra_color = MUTED
            elif state == STATE_AWAY:
                extra_text = "当前屏幕：已锁定 · 手机已离开，靠近后可点击解锁"
                extra_color = MUTED
            else:
                extra_text = "当前屏幕：已锁定 · 正在确认手机状态"
                extra_color = MUTED
        else:
            extra_text = "当前屏幕：已解锁" + (" · 远程桌面" if remote else "")
            extra_color = MUTED
        self._status_extra.configure(text=extra_text, text_color=extra_color)

        rssi = presence["rssi"]
        self._signal_value.configure(text=f"{rssi} dBm" if rssi is not None else "—")
        if not presence["scanning"]:
            note = presence["error"] or "蓝牙扫描未启动"
            note_color = RED if presence["error"] else MUTED
        elif rssi is None:
            if phone_bound:
                note = f"尚未收到「{phone_name}」的广播"
            else:
                note = "尚未收到手机广播，正在识别…"
            note_color = MUTED
        else:
            age = presence["last_seen_age"] or 0
            smoothed = presence["smoothed"]
            label = f"{source_label} · " if source_label else ""
            note = f"{label}实时 {rssi} dBm · 平滑 {smoothed} dBm · {age:.1f} 秒前"
            if presence["address"]:
                note += f" · {presence['address']}"
            note_color = MUTED
        self._signal_note.configure(text=note, text_color=note_color)

        needs_setup = self.controller.settings.click_unlock and not (
            snap["cp_installed"] and snap["has_password"]
        )
        if needs_setup and not self._setup_hint_visible:
            self._setup_hint.pack(fill="x", pady=(4, 0))
            self._setup_hint_visible = True
        elif not needs_setup and self._setup_hint_visible:
            self._setup_hint.pack_forget()
            self._setup_hint_visible = False

        self._auto_lock_var.set(self.controller.settings.auto_lock)
        self._click_unlock_var.set(self.controller.settings.click_unlock)
        self._pause_var.set(self.controller.settings.paused)
        self._autostart_var.set(autostart.is_enabled())

        installed = snap["cp_installed"]
        self._cp_status.configure(
            text="已安装" if installed else "未安装",
            text_color=GREEN if installed else AMBER,
        )
        self._install_btn.configure(text="重新安装" if installed else "安装组件")
        self._uninstall_btn.configure(state="normal" if installed else "disabled")

        has_password = snap["has_password"]
        self._password_status.configure(
            text="已保存" if has_password else "未保存",
            text_color=GREEN if has_password else AMBER,
        )
        self._password_btn.configure(text="修改密码" if has_password else "设置密码")
        self._clear_password_btn.configure(state="normal" if has_password else "disabled")

        if not installed and self.controller.settings.click_unlock:
            self._cp_note.configure(
                text="未安装组件：锁屏界面不会出现手机解锁选项", text_color=AMBER
            )
        else:
            self._cp_note.configure(
                text="安装后锁屏界面才会出现手机解锁选项", text_color=MUTED
            )

        self._action_label.configure(text=snap["last_action"] or "")
        self._error_label.configure(text=snap["last_error"] or "")

    def _set_status(self, title: str, detail: str, color) -> None:
        self._status_title.configure(text=title, text_color=color)
        self._status_detail.configure(text=detail)

    # ------------------------------------------------------------------
    # Actions
    # ------------------------------------------------------------------

    def _on_auto_lock(self) -> None:
        self.controller.settings.auto_lock = bool(self._auto_lock_var.get())
        save_settings(self.controller.settings)

    def _on_click_unlock(self) -> None:
        self.controller.settings.click_unlock = bool(self._click_unlock_var.get())
        save_settings(self.controller.settings)

    def _on_pause(self) -> None:
        self.controller.set_paused(bool(self._pause_var.get()))
        self.refresh_now()

    def _on_autostart(self) -> None:
        enabled = bool(self._autostart_var.get())
        autostart.set_enabled(enabled)
        self.controller.settings.autostart = enabled
        save_settings(self.controller.settings)
        if autostart.is_enabled() != enabled:
            messagebox.showerror(APP_TITLE, "写入开机启动项失败。", parent=self)
            self._autostart_var.set(autostart.is_enabled())

    def _open_password(self) -> None:
        PasswordDialog(self, self.refresh_now)

    def _clear_password(self) -> None:
        if not messagebox.askyesno(
            APP_TITLE, "清除已保存的 Windows 密码？\n\n手机解锁选项将不可用，直到重新设置密码。",
            parent=self,
        ):
            return
        secret.clear_secret()
        self.refresh_now()

    def _install_cp(self) -> None:
        if not messagebox.askyesno(
            APP_TITLE,
            "安装锁屏解锁组件需要管理员权限。\n\n"
            "它会把 UnlockProvider.dll 复制到 C:\\Windows\\System32，"
            "并在 Windows 注册表中注册为锁屏界面的凭据提供程序。\n\n"
            "组件仅用于在本机锁屏界面提交你保存的密码。可随时点击「卸载组件」移除。\n\n"
            "继续安装？",
            parent=self,
        ):
            return
        self._install_btn.configure(state="disabled", text="正在安装…")
        self._action_label.configure(text="正在安装解锁组件，请在弹出的 UAC 提示中允许。")

        def work() -> None:
            ok, message = cpsetup.install_cp()
            self.post(self._after_cp_change, ok, message, True)

        threading.Thread(target=work, name="install-cp", daemon=True).start()

    def _uninstall_cp(self) -> None:
        if not messagebox.askyesno(
            APP_TITLE,
            "卸载锁屏解锁组件？\n\n将删除 System32 中的 UnlockProvider.dll 并移除注册表项。"
            "锁屏界面将不再出现手机解锁选项。",
            parent=self,
        ):
            return
        self._uninstall_btn.configure(state="disabled", text="正在卸载…")
        self._action_label.configure(text="正在卸载解锁组件，请在弹出的 UAC 提示中允许。")

        def work() -> None:
            ok, message = cpsetup.uninstall_cp()
            self.post(self._after_cp_change, ok, message, False)

        threading.Thread(target=work, name="uninstall-cp", daemon=True).start()

    def _after_cp_change(self, ok: bool, message: str, installed: bool) -> None:
        self.controller.invalidate_cp_cache()
        self.refresh_now()
        if ok:
            self._action_label.configure(
                text="解锁组件已安装" if installed else "解锁组件已卸载"
            )
            messagebox.showinfo(
                APP_TITLE,
                "解锁组件已安装，下次锁屏时生效。" if installed else "解锁组件已卸载。",
                parent=self,
            )
        else:
            self._action_label.configure(text="")
            messagebox.showerror(
                APP_TITLE,
                f"操作失败：{message or '未知错误'}\n\n请确认已在 UAC 提示中允许管理员权限。",
                parent=self,
            )

    def _open_advanced(self) -> None:
        AdvancedDialog(
            self,
            self.controller.settings,
            self.refresh_now,
            self.controller.rebind_phone,
        )

    def _open_logs(self) -> None:
        try:
            os.startfile(str(log_path().parent))
        except OSError as exc:
            messagebox.showerror(APP_TITLE, f"无法打开日志文件夹：{exc}", parent=self)

    def _hide(self) -> None:
        self.withdraw()
        self._on_hide()


class PasswordDialog(ctk.CTkToplevel):
    def __init__(self, master: MainWindow, on_saved):
        super().__init__(master)
        self._on_saved = on_saved
        self._main = master
        self.title("设置 Windows 密码")
        self.geometry("500x400")
        self.resizable(False, True)
        self.transient(master)
        self.grab_set()

        font_title = ctk.CTkFont(family="Microsoft YaHei UI", size=18, weight="bold")
        font_small = ctk.CTkFont(family="Microsoft YaHei UI", size=12)
        font_body = ctk.CTkFont(family="Microsoft YaHei UI", size=13)

        frame = ctk.CTkFrame(self, fg_color="transparent")
        frame.pack(fill="both", expand=True, padx=24, pady=20)

        ctk.CTkLabel(
            frame, text="保存 Windows 密码", font=font_title, text_color=TEXT, anchor="w"
        ).pack(fill="x")
        ctk.CTkLabel(
            frame,
            text=f"账户：{credential.account_name()}。请填写 Windows 账户密码，不是 PIN，也不是 Windows Hello。密码会用 DPAPI 加密保存在本机，仅在锁屏界面点击手机解锁时提交。",
            font=font_small,
            text_color=MUTED,
            anchor="w",
            justify="left",
            wraplength=440,
        ).pack(fill="x", pady=(6, 14))

        ctk.CTkLabel(frame, text="密码", font=font_body, text_color=TEXT, anchor="w").pack(fill="x")
        self._password = ctk.CTkEntry(frame, show="•", height=34, font=font_body)
        self._password.pack(fill="x", pady=(4, 10))

        ctk.CTkLabel(frame, text="确认密码", font=font_body, text_color=TEXT, anchor="w").pack(fill="x")
        self._confirm = ctk.CTkEntry(frame, show="•", height=34, font=font_body)
        self._confirm.pack(fill="x", pady=(4, 10))

        self._message = ctk.CTkLabel(
            frame,
            text="",
            font=font_small,
            text_color=RED,
            anchor="w",
            justify="left",
            wraplength=440,
        )
        self._message.pack(fill="x")

        buttons = ctk.CTkFrame(frame, fg_color="transparent")
        buttons.pack(fill="x", pady=(10, 0))
        self._save_btn = ctk.CTkButton(
            buttons,
            text="保存并验证",
            width=110,
            height=34,
            corner_radius=8,
            fg_color=ACCENT,
            hover_color=ACCENT_HOVER,
            font=font_body,
            command=self._save,
        )
        self._save_btn.pack(side="right")
        ctk.CTkButton(
            buttons,
            text="取消",
            width=80,
            height=34,
            corner_radius=8,
            fg_color=("gray85", "gray25"),
            hover_color=("gray75", "gray35"),
            text_color=TEXT,
            font=font_body,
            command=self.destroy,
        ).pack(side="right", padx=(0, 10))

        self.after(80, self._password.focus_set)

    def _save(self) -> None:
        password = self._password.get()
        confirm = self._confirm.get()
        if not password:
            self._message.configure(text="请输入密码。")
            return
        if len(password) > secret.PASSWORD_MAX:
            self._message.configure(text="密码过长。")
            return
        if password != confirm:
            self._message.configure(text="两次输入的密码不一致。")
            return

        self._message.configure(text="正在验证…", text_color=MUTED)
        self._save_btn.configure(state="disabled", text="验证中…")

        def work() -> None:
            ok, error = credential.verify_password(
                credential.account_name(), password
            )
            save_error = ""
            if ok:
                try:
                    secret.save_secret(password)
                except (OSError, ValueError) as exc:
                    save_error = str(exc)
                    ok = False
            self._main.post(self._done, ok, error, save_error)

        threading.Thread(target=work, name="verify-password", daemon=True).start()

    def _done(self, ok: bool, error: str, save_error: str) -> None:
        self._password.delete(0, "end")
        self._confirm.delete(0, "end")
        if ok:
            self._message.configure(text="已保存并验证通过。", text_color=GREEN)
            self._save_btn.configure(state="disabled", text="完成")
            self._on_saved()
            self.after(700, self.destroy)
        else:
            message = save_error or error or "验证失败"
            self._message.configure(text=f"验证失败：{message}", text_color=RED)
            self._save_btn.configure(state="normal", text="保存并验证")


class AdvancedDialog(ctk.CTkToplevel):
    def __init__(self, master: MainWindow, settings, on_saved, on_rebind):
        super().__init__(master)
        self._settings = settings
        self._on_saved = on_saved
        self._on_rebind = on_rebind
        self._labels: dict[str, ctk.CTkLabel] = {}
        self._vars: dict[str, tk.DoubleVar] = {}

        self.title("高级设置")
        self.geometry("560x540")
        self.resizable(False, True)
        self.transient(master)
        self.grab_set()

        font_title = ctk.CTkFont(family="Microsoft YaHei UI", size=18, weight="bold")
        font_small = ctk.CTkFont(family="Microsoft YaHei UI", size=12)
        font_body = ctk.CTkFont(family="Microsoft YaHei UI", size=13)

        frame = ctk.CTkFrame(self, fg_color="transparent")
        frame.pack(fill="both", expand=True, padx=24, pady=20)
        ctk.CTkLabel(
            frame, text="时间设置", font=font_title, text_color=TEXT, anchor="w"
        ).pack(fill="x")
        ctk.CTkLabel(
            frame,
            text="距离判定由应用自动完成，无需调整。修改保存后立即生效。",
            font=font_small,
            text_color=MUTED,
            anchor="w",
            justify="left",
            wraplength=500,
        ).pack(fill="x", pady=(4, 8))

        scroll = ctk.CTkScrollableFrame(frame, fg_color="transparent", corner_radius=0)
        scroll.pack(fill="both", expand=True)
        self._scroll = scroll

        self._slider(scroll, "lock_grace", "自动锁屏延迟", "手机离开确认后，等待多久再锁屏（需开启自动锁屏）", 0, 30, 30, "{:.0f} 秒")
        self._slider(scroll, "unlock_delay", "解锁选项延迟", "锁屏后手机在附近时，等待多久显示解锁选项", 0, 10, 20, "{:.1f} 秒")
        self._phone_section(scroll)

        buttons = ctk.CTkFrame(frame, fg_color="transparent")
        buttons.pack(fill="x", pady=(10, 0))
        ctk.CTkButton(
            buttons,
            text="恢复默认",
            width=90,
            height=32,
            corner_radius=8,
            fg_color="transparent",
            border_width=1,
            border_color=LINE,
            hover_color=("gray90", "gray22"),
            text_color=TEXT,
            font=font_small,
            command=self._reset,
        ).pack(side="left")
        ctk.CTkButton(
            buttons,
            text="保存",
            width=100,
            height=32,
            corner_radius=8,
            fg_color=ACCENT,
            hover_color=ACCENT_HOVER,
            font=font_body,
            command=self._save,
        ).pack(side="right")
        ctk.CTkButton(
            buttons,
            text="取消",
            width=80,
            height=32,
            corner_radius=8,
            fg_color=("gray85", "gray25"),
            hover_color=("gray75", "gray35"),
            text_color=TEXT,
            font=font_body,
            command=self.destroy,
        ).pack(side="right", padx=(0, 10))

        self._sync_labels()

    def _slider(self, parent, key, title, desc, lo, hi, steps, fmt) -> None:
        row = ctk.CTkFrame(parent, fg_color="transparent")
        row.pack(fill="x", pady=(4, 10))
        head = ctk.CTkFrame(row, fg_color="transparent")
        head.pack(fill="x")
        ctk.CTkLabel(
            head, text=title, font=("Microsoft YaHei UI", 13, "bold"), text_color=TEXT, anchor="w"
        ).pack(side="left")
        value_label = ctk.CTkLabel(
            head, text="", font=("Microsoft YaHei UI", 13, "bold"), text_color=ACCENT
        )
        value_label.pack(side="right")
        ctk.CTkLabel(
            row,
            text=desc,
            font=("Microsoft YaHei UI", 11),
            text_color=MUTED,
            anchor="w",
            justify="left",
            wraplength=480,
        ).pack(fill="x")
        var = tk.DoubleVar(value=getattr(self._settings, key))
        slider = ctk.CTkSlider(
            row,
            from_=lo,
            to=hi,
            number_of_steps=steps,
            variable=var,
            command=lambda value, k=key, f=fmt: self._update_label(k, value, f),
            progress_color=ACCENT,
            height=18,
        )
        slider.pack(fill="x", pady=(4, 0))
        # CTkSlider changes its value on mouse wheel and CTkScrollableFrame ignores
        # wheel events over sliders, so scroll the page ourselves.
        for sequence in ("<MouseWheel>", "<Button-4>", "<Button-5>"):
            slider._canvas.unbind(sequence)
        slider._canvas.bind("<MouseWheel>", self._on_slider_wheel)
        self._labels[key] = value_label
        self._vars[key] = var
        self._labels[key].configure(text=fmt.format(getattr(self._settings, key)))

    def _on_slider_wheel(self, event) -> str:
        self._scroll._parent_canvas.yview_scroll(-int(event.delta / 6), "units")
        return "break"

    def _update_label(self, key: str, value: float, fmt: str) -> None:
        self._labels[key].configure(text=fmt.format(value))

    def _sync_labels(self) -> None:
        formats = {
            "lock_grace": "{:.0f} 秒",
            "unlock_delay": "{:.1f} 秒",
        }
        for key, label in self._labels.items():
            label.configure(text=formats[key].format(self._vars[key].get()))

    def _phone_section(self, parent) -> None:
        ctk.CTkFrame(parent, height=1, fg_color=LINE).pack(fill="x", pady=(8, 14))
        ctk.CTkLabel(
            parent,
            text="手机",
            font=("Microsoft YaHei UI", 14, "bold"),
            text_color=TEXT,
            anchor="w",
        ).pack(fill="x")
        if self._settings.phone_adv_prefix or self._settings.phone_adv_address:
            detail = f"已绑定「{self._settings.phone_adv_name or '未命名手机'}」"
        else:
            detail = "尚未绑定，应用会自动识别手机广播。"
        self._phone_status = ctk.CTkLabel(
            parent,
            text=detail,
            font=("Microsoft YaHei UI", 12),
            text_color=MUTED,
            anchor="w",
            justify="left",
            wraplength=480,
        )
        self._phone_status.pack(fill="x", pady=(4, 0))
        ctk.CTkLabel(
            parent,
            text="保持手机蓝牙开启即可。识别不到或更换手机时，可重新绑定。",
            font=("Microsoft YaHei UI", 11),
            text_color=MUTED,
            anchor="w",
            justify="left",
            wraplength=480,
        ).pack(fill="x", pady=(4, 0))
        self._rebind_btn = ctk.CTkButton(
            parent,
            text="重新绑定手机",
            width=130,
            height=32,
            corner_radius=8,
            fg_color="transparent",
            border_width=1,
            border_color=LINE,
            hover_color=("gray90", "gray22"),
            text_color=TEXT,
            font=("Microsoft YaHei UI", 12),
            command=self._rebind,
        )
        self._rebind_btn.pack(anchor="w", pady=(8, 4))

    def _rebind(self) -> None:
        self._on_rebind()
        self._rebind_btn.configure(state="disabled", text="正在重新识别…")
        self._phone_status.configure(
            text="已清除绑定，正在识别手机广播；请保持手机蓝牙开启。",
            text_color=GREEN,
        )

    def _reset(self) -> None:
        for key in self._vars:
            self._vars[key].set(getattr(DEFAULT_SETTINGS, key))
        self._sync_labels()

    def _save(self) -> None:
        settings = self._settings
        settings.lock_grace = int(round(self._vars["lock_grace"].get()))
        settings.unlock_delay = round(self._vars["unlock_delay"].get(), 1)
        settings.normalize()
        save_settings(settings)
        self._on_saved()
        self.destroy()

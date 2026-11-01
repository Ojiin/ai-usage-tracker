"""AI usage widget: a small always-on-top window you can drag anywhere.

Left-drag moves it, right-click opens the menu. It rescans every 5 minutes and
regenerates dashboard.html next to the exe/script, which the ↗ button opens.
"""
import ctypes
import json
import os
import queue
import threading
import tkinter as tk
import traceback
import webbrowser
from datetime import datetime
from pathlib import Path

import api_billing
import limits_watch
import startup
import tracker
from tray import WindowsTray

REFRESH_MS = 5 * 60 * 1000
COUNTDOWN_MS = 30 * 1000  # reset countdowns tick between scans
HOVER_POLL_MS = 150  # how often an open hover card checks the pointer is still on its label
SETTINGS = Path(os.environ.get("LOCALAPPDATA", Path.home())) / "AIUsageTracker" / "widget.json"
DASHBOARD = tracker.HERE / "dashboard.html"

BG, CARD, TEXT, MUTED, TRACK = "#15181d", "#1d2128", "#e6e8eb", "#98a2b3", "#2a2f37"
WARN = "#f79009"
TOOL_COLORS = {"Claude Code": "#e8916f", "Codex": "#5b8ff0"}
API_COLORS = {"Anthropic": "#e8916f", "OpenAI": "#10a37f"}
COST_TOOLS = ("Claude Code", "Codex")  # tools whose API-equivalent dollar cost gets its own block
FONT = "Segoe UI"


def fmt(n):
    for size, suffix in ((1e9, "B"), (1e6, "M"), (1e3, "k")):
        if n >= size:
            return f"{n / size:.{2 if suffix == 'B' else 1}f}{suffix}"
    return str(int(n))


def until(seconds):
    """Compact time left: '2h 14m', '3d 4h', '12m'."""
    m = max(int(seconds) // 60, 0)
    if m >= 1440:
        return f"{m // 1440}d {m % 1440 // 60}h"
    return f"{m // 60}h {m % 60:02d}m" if m >= 60 else f"{m}m"


def money(n):
    return f"-${-n:,.2f}" if n < 0 else f"${n:,.2f}"


def load_settings():
    try:
        return json.loads(SETTINGS.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def save_settings(data):
    try:
        SETTINGS.parent.mkdir(parents=True, exist_ok=True)
        SETTINGS.write_text(json.dumps(data), encoding="utf-8")
    except OSError:
        pass


def virtual_screen():
    """(left, top, right, bottom) across all monitors."""
    try:
        m = ctypes.windll.user32.GetSystemMetrics
        x, y = m(76), m(77)
        return x, y, x + m(78), y + m(79)
    except (AttributeError, OSError):
        return None


class Widget:
    def __init__(self):
        self.settings = load_settings()
        self.results = queue.Queue()
        self.scanning = False
        self.tray_events = queue.Queue()
        self.tray = WindowsTray(self.tray_events)

        self.root = root = tk.Tk()
        root.title("AI Usage")
        root.overrideredirect(True)
        root.configure(bg=BG)
        root.attributes("-alpha", 0.96)
        self.topmost = tk.BooleanVar(value=self.settings.get("topmost", True))
        root.attributes("-topmost", self.topmost.get())

        frame = tk.Frame(root, bg=CARD, padx=14, pady=10, highlightthickness=1, highlightbackground=TRACK)
        frame.pack(fill="both", expand=True)

        head = tk.Frame(frame, bg=CARD)
        head.pack(fill="x")
        tk.Label(head, text="AI Usage", bg=CARD, fg=TEXT, font=(FONT, 10, "bold")).pack(side="left")
        self.working = tk.Label(head, text="", bg=CARD, fg="#12b76a", font=(FONT, 8))
        self.working.pack(side="left", padx=(8, 0))
        self.working.bind("<Enter>", self._show_working)
        for symbol, tip, cmd in (("✕", "Close", self.quit), ("−", "Hide to tray", self.hide_to_tray), ("↗", "Open dashboard", self.open_dashboard), ("⟳", "Refresh", self.refresh)):
            b = tk.Label(head, text=symbol, bg=CARD, fg=MUTED, font=(FONT, 11), cursor="hand2", padx=4)
            b.pack(side="right")
            b.bind("<Button-1>", lambda e, c=cmd: c())
            b.bind("<Enter>", lambda e, w=b: w.configure(fg=TEXT))
            b.bind("<Leave>", lambda e, w=b: w.configure(fg=MUTED))
            b.is_button = True

        self.today = self._stat(frame, "TODAY")
        self.today_split = tk.Frame(frame, bg=CARD)
        self.today_split.pack(fill="x")
        self.month = self._stat(frame, "THIS MONTH")
        self.month_sub = tk.Label(frame, bg=CARD, fg=MUTED, font=(FONT, 8), anchor="w")
        self.month_sub.pack(fill="x")
        self.costs = tk.Frame(frame, bg=CARD)
        self.costs.pack(fill="x")

        self.limits = tk.Frame(frame, bg=CARD)
        self.limits.pack(fill="x")
        self.api = tk.Frame(frame, bg=CARD)
        self.api.pack(fill="x")
        self.last_data = None

        self.status = tk.Label(frame, text="Scanning…", bg=CARD, fg=MUTED, font=(FONT, 8), anchor="w")
        self.status.pack(fill="x", pady=(10, 0))

        self.menu = tk.Menu(root, tearoff=0)
        self.menu.add_command(label="Hide to tray (keep tracking)", command=self.hide_to_tray)
        self.menu.add_command(label="Refresh now", command=self.refresh)
        self.menu.add_command(label="Open dashboard", command=self.open_dashboard)
        self.menu.add_command(label="Set API balance…", command=self.balance_dialog)
        self.menu.add_checkbutton(label="Always on top", variable=self.topmost, command=self.toggle_topmost)
        self.alerts = tk.BooleanVar(value=self.settings.get("alerts", True))
        self.menu.add_checkbutton(label="Limit alerts", variable=self.alerts, command=self.toggle_alerts)
        try:
            startup.refresh()
        except OSError:
            pass
        self.autostart = tk.BooleanVar(value=startup.is_enabled())
        self.menu.add_checkbutton(label="Start with Windows", variable=self.autostart, command=self.toggle_autostart)
        self.menu.add_separator()
        self.menu.add_command(label="Exit", command=self.quit)

        self._bind_all(root)
        root.geometry("+{}+{}".format(*self._start_position()))
        self.tray.start()
        root.after(200, self.poll)
        self.refresh()
        root.after(REFRESH_MS, self.auto_refresh)
        self.countdowns = []
        self._working()
        root.after(COUNTDOWN_MS, self.tick)

    def _stat(self, parent, label):
        tk.Label(parent, text=label, bg=CARD, fg=MUTED, font=(FONT, 8), anchor="w").pack(fill="x", pady=(8, 0))
        value = tk.Label(parent, text="—", bg=CARD, fg=TEXT, font=(FONT, 18, "bold"), anchor="w")
        value.pack(fill="x")
        return value

    # --- moving -----------------------------------------------------------
    def _bind_all(self, widget):
        if not getattr(widget, "is_button", False):
            widget.bind("<ButtonPress-1>", self._drag_start)
            widget.bind("<B1-Motion>", self._drag_move)
            widget.bind("<ButtonRelease-1>", self._drag_end)
        widget.bind("<Button-3>", lambda e: self.menu.tk_popup(e.x_root, e.y_root))
        for child in widget.winfo_children():
            self._bind_all(child)

    def _drag_start(self, e):
        self._offset = (e.x_root - self.root.winfo_x(), e.y_root - self.root.winfo_y())

    def _drag_move(self, e):
        self.root.geometry(f"+{e.x_root - self._offset[0]}+{e.y_root - self._offset[1]}")

    def _drag_end(self, e):
        self.settings["x"], self.settings["y"] = self.root.winfo_x(), self.root.winfo_y()
        save_settings(self.settings)

    def _start_position(self):
        x, y = self.settings.get("x"), self.settings.get("y")
        screen = virtual_screen()
        if x is None or y is None or (screen and not (screen[0] <= x < screen[2] - 40 and screen[1] <= y < screen[3] - 40)):
            # default / off-screen (monitor unplugged): top-right of the primary screen
            return self.root.winfo_screenwidth() - 300, 60
        return x, y

    # --- data -------------------------------------------------------------
    def refresh(self):
        if self.scanning:
            return
        self.scanning = True
        self.status.configure(text="Scanning…")
        threading.Thread(target=self._scan, daemon=True).start()

    def auto_refresh(self):
        self.refresh()
        self.root.after(REFRESH_MS, self.auto_refresh)

    def _scan(self):
        try:
            data = tracker.collect(log=lambda *a: None)
            tracker.write_dashboard(data, DASHBOARD)
            self.results.put(("ok", data))
        except Exception:
            self.results.put(("error", traceback.format_exc(limit=1).strip().splitlines()[-1]))

    def poll(self):
        # Tray callbacks run on a Windows message thread; all Tk work stays here.
        while True:
            try:
                action = self.tray_events.get_nowait()
            except queue.Empty:
                break
            if isinstance(action, tuple):
                self.restore()
                self.status.configure(text=action[1])
            elif action == "quit":
                self.quit()
                return
            else:
                {"show": self.restore, "refresh": self.refresh,
                 "dashboard": self.open_dashboard}[action]()
        try:
            kind, payload = self.results.get_nowait()
        except queue.Empty:
            pass
        else:
            self.scanning = False
            if kind == "ok":
                self.show(payload)
            else:
                self.status.configure(text=f"Scan failed: {payload}"[:60])
        self.root.after(200, self.poll)

    def show(self, data):
        today = data["daily_tool"].get(data["today"], {})
        self.today.configure(text=fmt(sum(today.values())))
        for w in self.today_split.winfo_children():
            w.destroy()
        for tool, color in TOOL_COLORS.items():
            tk.Label(self.today_split, text="●", bg=CARD, fg=color, font=(FONT, 8)).pack(side="left")
            tk.Label(self.today_split, text=f"{tool} {fmt(today.get(tool, 0))}  ", bg=CARD, fg=MUTED, font=(FONT, 8)).pack(side="left")

        month_start = data["today"][:8] + "01"
        rows = [r for r in data["day"] if r["key"] >= month_start]
        self.month.configure(text=fmt(sum(r["total"] for r in rows)))
        cost = sum(r["cost"] for r in rows)
        self.month_sub.configure(text=f'{sum(r["calls"] for r in rows):,} usage records' + (f" · {money(cost)} at API prices" if data["has_pricing"] else ""))

        for w in self.costs.winfo_children():
            w.destroy()
        for tool in COST_TOOLS:
            self._tool_cost(tool, data)

        for w in self.limits.winfo_children():
            w.destroy()
        self.countdowns = []
        for entry in data.get("plan_limits", []):
            self._service_limits(entry, data["now_epoch"])
        self.last_data = data
        # Anthropic has no account token history, so Claude's line comes from this PC's logs.
        claude = next((t for t in data.get("tool", []) if t["key"] == "Claude Code"), None)
        if claude:
            days = {d: t["Claude Code"] for d, t in data.get("daily_tool", {}).items() if t.get("Claude Code")}
            latest = max(days) if days else None
            text = f"Claude Code (this PC): {fmt(claude['total'])} lifetime tokens"
            if latest:
                text += f"\nLatest day {latest}: {fmt(days[latest])}"
            tk.Label(self.limits, text=text, bg=CARD, fg=MUTED, font=(FONT, 8),
                     anchor="w", justify="left", wraplength=260).pack(fill="x", pady=(6, 0))
        account = data.get("openai_analytics") or {}
        lifetime = (account.get("summary") or {}).get("lifetimeTokens")
        if lifetime is not None:
            days = account.get("daily") or {}
            latest = max(days) if days else None
            text = f"OpenAI account: {fmt(lifetime)} lifetime tokens"
            if latest:
                text += f"\nLatest day {latest}: {fmt(days[latest])}"
            tk.Label(self.limits, text=text, bg=CARD, fg=MUTED, font=(FONT, 8),
                     anchor="w", justify="left", wraplength=260).pack(fill="x", pady=(6, 0))
        self._api_section(data.get("api") or [])

        self.status.configure(text=f"Updated {datetime.now():%H:%M}" + (" · incomplete logs" if data.get("warnings") else ""), fg=MUTED)
        self._alerts(data)
        self._bind_all(self.root)  # new labels need drag/menu bindings

    def _tool_cost(self, tool, data):
        """What this tool's tokens would cost at API prices (pricing.json): today and this month."""
        head = tk.Frame(self.costs, bg=CARD)
        head.pack(fill="x", pady=(10, 0))
        tk.Label(head, text="●", bg=CARD, fg=TOOL_COLORS.get(tool, MUTED), font=(FONT, 8)).pack(side="left")
        tk.Label(head, text=f"{tool.upper()} COST (API-EQUIVALENT)", bg=CARD, fg=MUTED, font=(FONT, 8)).pack(side="left")
        if not data.get("has_pricing"):
            tk.Label(self.costs, text="Add prices to pricing.json to see dollars", bg=CARD, fg=MUTED,
                     font=(FONT, 8), anchor="w").pack(fill="x")
            return
        today, month_start = data["today"], data["today"][:8] + "01"
        daily = data.get("daily_tool_cost", {})
        today_cost = daily.get(today, {}).get(tool, 0.0)
        month_cost = sum(t.get(tool, 0.0) for d, t in daily.items() if d >= month_start)
        row = tk.Frame(self.costs, bg=CARD)
        row.pack(fill="x")
        tk.Label(row, text=money(month_cost), bg=CARD, fg=TEXT, font=(FONT, 16, "bold")).pack(side="left")
        tk.Label(row, text=" this month", bg=CARD, fg=MUTED, font=(FONT, 8)).pack(side="left", anchor="s", pady=(0, 4))
        tk.Label(row, text=f"today {money(today_cost)}", bg=CARD, fg=TEXT, font=(FONT, 9)).pack(side="right", anchor="s", pady=(0, 4))
        unpriced = any(tool in tools for d, tools in data.get("daily_tool_unpriced", {}).items() if d >= month_start)
        if unpriced:
            tk.Label(self.costs, text="some models have no price in pricing.json and aren't counted", bg=CARD,
                     fg="#f79009", font=(FONT, 8), anchor="w", wraplength=240, justify="left").pack(fill="x")

    def _service_limits(self, entry, now):
        service = entry["service"]
        head = tk.Frame(self.limits, bg=CARD)
        head.pack(fill="x", pady=(10, 0))
        tk.Label(head, text="●", bg=CARD, fg=TOOL_COLORS.get(service, MUTED), font=(FONT, 8)).pack(side="left")
        tk.Label(head, text=f"{service.upper()} LIMITS", bg=CARD, fg=MUTED, font=(FONT, 8)).pack(side="left")
        if entry.get("plan"):
            tk.Label(head, text=entry["plan"], bg=CARD, fg=MUTED, font=(FONT, 8)).pack(side="right")
        windows = entry.get("windows") or []
        if not windows:
            tk.Label(self.limits, text="No limit data yet", bg=CARD, fg=MUTED, font=(FONT, 8), anchor="w").pack(fill="x")
        for w in windows:
            self._limit_bar(w, now)
        for detail in entry.get("details", []):
            tk.Label(self.limits, text=detail, bg=CARD, fg=MUTED, font=(FONT, 8),
                     anchor="w", wraplength=240, justify="left").pack(fill="x")
        if windows and entry.get("as_of"):
            seen = datetime.fromisoformat(entry["as_of"].replace("Z", "+00:00")).astimezone()
            note = {"live": f"live, checked {seen:%H:%M}",
                    "cached": f"last live check {seen:%a %H:%M}",
                    "log": f"from last Codex turn, {seen:%a %H:%M}"}.get(entry.get("source"), "")
            tk.Label(self.limits, text=note, bg=CARD, fg=MUTED, font=(FONT, 8, "italic"), anchor="w").pack(fill="x", pady=(2, 0))
        if entry.get("error"):
            tk.Label(self.limits, text=entry["error"], bg=CARD, fg="#f79009", font=(FONT, 8),
                     anchor="w", wraplength=240, justify="left").pack(fill="x")

    def _api_section(self, providers):
        for w in self.api.winfo_children():
            w.destroy()
        small = lambda text, fg=MUTED, **kw: tk.Label(self.api, text=text, bg=CARD, fg=fg, font=(FONT, 8), anchor="w",
                                                       wraplength=240, justify="left", **kw).pack(fill="x")
        configured = [p for p in providers if p.get("configured")]
        if not configured:
            tk.Label(self.api, text="API SPEND", bg=CARD, fg=MUTED, font=(FONT, 8), anchor="w").pack(fill="x", pady=(10, 0))
            small("Set ANTHROPIC_ADMIN_KEY or OPENAI_ADMIN_KEY to track API spend (see README)")
            return
        for p in configured:
            head = tk.Frame(self.api, bg=CARD)
            head.pack(fill="x", pady=(10, 0))
            tk.Label(head, text="●", bg=CARD, fg=API_COLORS.get(p["provider"], MUTED), font=(FONT, 8)).pack(side="left")
            tk.Label(head, text=f'{p["provider"].upper()} API', bg=CARD, fg=MUTED, font=(FONT, 8)).pack(side="left")
            if p.get("month") is not None:
                tk.Label(head, text=f'{money(p["month"])} this month', bg=CARD, fg=TEXT, font=(FONT, 9, "bold")).pack(side="right")
                small(f'Today {money(p.get("today", 0))} · last 30 days {money(p.get("last30", 0))} (UTC days)')
            bal = p.get("balance")
            if bal:
                fg = "#d92d20" if bal["remaining"] < 5 else TEXT
                tk.Label(self.api, text=f'Balance ≈ {money(bal["remaining"])}', bg=CARD, fg=fg,
                         font=(FONT, 10, "bold"), anchor="w").pack(fill="x", pady=(2, 0))
                when = datetime.fromisoformat(bal["as_of"].replace("Z", "+00:00")).astimezone()
                small(f'estimate: {money(bal["entered"])} entered {when:%b %d} − {money(bal["spent_since"])} spent since')
            else:
                small("Right-click → Set API balance… to track remaining credit")
            if p.get("source") == "cached" and p.get("as_of"):
                when = datetime.fromisoformat(p["as_of"].replace("Z", "+00:00")).astimezone()
                small(f"last good report {when:%a %H:%M}")
            if p.get("error"):
                small(p["error"], fg="#f79009")

    def balance_dialog(self):
        """Ask for the balance shown on the provider's billing page right now."""
        dlg = tk.Toplevel(self.root, bg=CARD, padx=14, pady=12)
        dlg.title("Set API balance")
        dlg.attributes("-topmost", True)
        dlg.resizable(False, False)
        providers = list(api_billing.PROVIDERS)
        choice = tk.StringVar(value=providers[0])
        amount = tk.StringVar()
        lbl = lambda text: tk.Label(dlg, text=text, bg=CARD, fg=MUTED, font=(FONT, 9), anchor="w", justify="left")
        lbl("Enter the credit balance your billing page shows now.\nThe widget subtracts spend reported after this.").grid(row=0, column=0, columnspan=3, sticky="w")
        lbl("Provider").grid(row=1, column=0, sticky="w", pady=(8, 0))
        menu = tk.OptionMenu(dlg, choice, *providers)
        menu.grid(row=1, column=1, columnspan=2, sticky="ew", pady=(8, 0))
        lbl("Balance (USD)").grid(row=2, column=0, sticky="w", pady=(6, 0))
        entry = tk.Entry(dlg, textvariable=amount, width=12)
        entry.grid(row=2, column=1, columnspan=2, sticky="ew", pady=(6, 0))
        error = tk.Label(dlg, text="", bg=CARD, fg="#f79009", font=(FONT, 8))
        error.grid(row=3, column=0, columnspan=3, sticky="w")

        def today_spend(provider):
            for p in (self.last_data or {}).get("api") or []:
                if p.get("provider") == provider:
                    return (p.get("daily") or {}).get(api_billing.utc_day(datetime.now().astimezone()))
            return None

        def save(clear=False):
            provider = choice.get()
            if clear:
                api_billing.set_balance(provider, None)
            else:
                try:
                    value = float(amount.get().replace("$", "").replace(",", "").strip())
                except ValueError:
                    error.configure(text="Enter a number, e.g. 25.00")
                    return
                api_billing.set_balance(provider, value, today_spend(provider))
            dlg.destroy()
            self.refresh()

        buttons = tk.Frame(dlg, bg=CARD)
        buttons.grid(row=4, column=0, columnspan=3, sticky="e", pady=(10, 0))
        tk.Button(buttons, text="Clear", command=lambda: save(clear=True)).pack(side="left", padx=(0, 6))
        tk.Button(buttons, text="Cancel", command=dlg.destroy).pack(side="left", padx=(0, 6))
        tk.Button(buttons, text="Save", command=save, default="active").pack(side="left")
        dlg.bind("<Return>", lambda e: save())
        dlg.bind("<Escape>", lambda e: dlg.destroy())
        dlg.geometry(f"+{self.root.winfo_x()}+{self.root.winfo_y() + 40}")
        entry.focus_set()

    def _limit_bar(self, w, now):
        expired = w.get("resets_at") and w["resets_at"] < now
        pct = float(w.get("used_percent") or 0)
        reset = "Reading expired; current usage unknown" if expired else datetime.fromtimestamp(w["resets_at"]).strftime("resets %a %H:%M") if w.get("resets_at") else ""
        row = tk.Frame(self.limits, bg=CARD)
        row.pack(fill="x", pady=(4, 0))
        tk.Label(row, text=w.get("label", "?"), bg=CARD, fg=TEXT, font=(FONT, 9)).pack(side="left")
        tk.Label(row, text=f"{'Last ' if expired else ''}{pct:.0f}%", bg=CARD, fg=TEXT, font=(FONT, 9, "bold")).pack(side="right")
        bar = tk.Canvas(self.limits, height=6, bg=TRACK, highlightthickness=0)
        bar.pack(fill="x", pady=(2, 0))
        color = "#d92d20" if pct >= 90 else "#f79009" if pct >= 70 else "#12b76a"
        bar.bind("<Configure>", lambda e: (bar.delete("all"), bar.create_rectangle(0, 0, e.width * min(pct, 100) / 100, 6, fill=color, width=0)))
        reset_label = tk.Label(self.limits, text=reset, bg=CARD, fg=MUTED, font=(FONT, 8), anchor="w")
        reset_label.pack(fill="x")
        if w.get("resets_at") and not expired:
            self.countdowns.append((reset_label, reset, w["resets_at"]))
            self._countdown(reset_label, reset, w["resets_at"])
        p = w.get("projection")
        if p:
            tk.Label(self.limits, text=limits_watch.pace_text(p, lambda t: datetime.fromtimestamp(t).strftime("%H:%M")),
                     bg=CARD, fg=WARN if "hits_at" in p else MUTED, font=(FONT, 8), anchor="w").pack(fill="x")

    def _countdown(self, label, reset, resets_at):
        left = resets_at - datetime.now().timestamp()
        label.configure(text=f"{reset} · in {until(left)}" if left > 0 else "Reset passed; refreshing soon")

    def tick(self):
        for label, reset, resets_at in self.countdowns:
            if label.winfo_exists():
                self._countdown(label, reset, resets_at)
        self._working()
        self.root.after(COUNTDOWN_MS, self.tick)

    def _working(self):
        """Header dot: sessions whose log changed in the last 2 minutes (subagents count with their parent)."""
        try:
            active = {t: n for t, n in tracker.working_now().items() if n}
        except OSError:
            active = {}
        total = sum(active.values())
        self.working.configure(text=f"● {total} working" if total else "")

    def _show_working(self, _event=None):
        """Hover card: each working session's model, effort, project, title, last prompt and subagents."""
        if getattr(self, "tip", None) is not None:
            return  # already open; opening it can re-send <Enter> to the label
        try:
            sessions = tracker.working_details()
        except OSError:
            sessions = []
        if not sessions:
            return
        tip = self.tip = tk.Toplevel(self.root)
        tip.overrideredirect(True)
        tip.attributes("-topmost", True)
        body = tk.Frame(tip, bg=CARD, padx=10, pady=8, highlightthickness=1, highlightbackground=TRACK)
        body.pack()
        for i, info in enumerate(sessions):
            first, *rest = tracker.describe_session(info).split("\n")
            row = tk.Frame(body, bg=CARD)
            row.pack(fill="x", pady=(8 if i else 0, 0))
            tk.Label(row, text="●", bg=CARD, fg=TOOL_COLORS.get(info["tool"], MUTED), font=(FONT, 8)).pack(side="left")
            tk.Label(row, text=first, bg=CARD, fg=TEXT, font=(FONT, 9, "bold")).pack(side="left")
            for line in rest:
                tk.Label(body, text=line.strip(), bg=CARD, fg=MUTED, font=(FONT, 8), anchor="w", justify="left",
                         wraplength=380).pack(fill="x", padx=(12, 0))
        tip.update_idletasks()
        left, top, right, bottom = virtual_screen() or (0, 0, self.root.winfo_screenwidth(), self.root.winfo_screenheight())
        x = min(self.working.winfo_rootx(), right - tip.winfo_width())
        y = self.working.winfo_rooty() + self.working.winfo_height() + 4
        if y + tip.winfo_height() > bottom:  # no room below: open above the label
            y = self.working.winfo_rooty() - tip.winfo_height() - 4
        tip.geometry(f"+{max(x, left)}+{max(y, top)}")
        self.root.after(HOVER_POLL_MS, self._track_hover)

    def _track_hover(self):
        # Windows sends the label a <Leave> as soon as the card window appears, even with the pointer
        # still on it, so the card closes only once the pointer is really somewhere else.
        if getattr(self, "tip", None) is None:
            return
        x, y = self.root.winfo_pointerxy()
        if self.root.winfo_containing(x, y) is self.working:
            self.root.after(HOVER_POLL_MS, self._track_hover)
        else:
            self._hide_working()

    def _hide_working(self, _event=None):
        if getattr(self, "tip", None) is not None:
            self.tip.destroy()
            self.tip = None

    def _alerts(self, data):
        # State advances even with alerts off, so turning them on doesn't replay old crossings.
        alerts = limits_watch.due_alerts(data.get("plan_limits", []), data["now_epoch"])
        if not alerts or not self.alerts.get():
            return
        for title, text in alerts:
            self.tray.notify(title, text)
        # Focus Assist can swallow the toast; the status line keeps the latest one visible.
        title, text = alerts[-1]
        self.status.configure(text=f"⚠ {title}", fg=WARN)

    # --- actions ----------------------------------------------------------
    def hide_to_tray(self):
        if self.tray.available:
            self.root.withdraw()
        else:
            self.status.configure(text="Tray icon unavailable — please try again")

    def restore(self):
        self.root.geometry("+{}+{}".format(*self._start_position()))
        self.root.deiconify()
        self.root.lift()

    def open_dashboard(self):
        if DASHBOARD.exists():
            webbrowser.open(DASHBOARD.as_uri())
        else:
            self.status.configure(text="Dashboard not built yet — wait for the scan")

    def toggle_topmost(self):
        self.root.attributes("-topmost", self.topmost.get())
        self.settings["topmost"] = self.topmost.get()
        save_settings(self.settings)

    def toggle_alerts(self):
        self.settings["alerts"] = self.alerts.get()
        save_settings(self.settings)

    def toggle_autostart(self):
        try:
            startup.enable() if self.autostart.get() else startup.disable()
            self.status.configure(text="Will start with Windows" if self.autostart.get() else "Won't start with Windows", fg=MUTED)
        except OSError as error:
            self.status.configure(text=f"Couldn't change startup: {error.strerror or error}"[:60], fg=WARN)
        self.autostart.set(startup.is_enabled())  # show what Windows actually has

    def quit(self):
        self.tray.stop()
        self.root.destroy()

    def run(self):
        self.root.minsize(240, 0)
        self.root.mainloop()


if __name__ == "__main__":
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(1)  # crisp text on scaled displays
    except (AttributeError, OSError):
        pass
    Widget().run()

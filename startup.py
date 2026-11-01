"""Start the tracker when you sign in to Windows, through the per-user Run key (no admin needed).

Only this app's own value is read or written. If the exe has moved since the option was turned on,
launching it from the new place updates the entry.
"""
import sys
import winreg
from pathlib import Path

RUN_KEY = r"Software\Microsoft\Windows\CurrentVersion\Run"
NAME = "AIUsageTracker"


def command():
    """What Windows should run: the exe itself, or pythonw with widget.py when running from source."""
    if getattr(sys, "frozen", False):
        return f'"{sys.executable}"'
    pythonw = Path(sys.executable).with_name("pythonw.exe")
    runner = pythonw if pythonw.exists() else Path(sys.executable)
    return f'"{runner}" "{Path(__file__).resolve().with_name("widget.py")}"'


def current():
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY) as key:
            return winreg.QueryValueEx(key, NAME)[0]
    except OSError:
        return None


def is_enabled():
    return current() is not None


def enable():
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, RUN_KEY) as key:
        winreg.SetValueEx(key, NAME, 0, winreg.REG_SZ, command())


def disable():
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY, 0, winreg.KEY_SET_VALUE) as key:
            winreg.DeleteValue(key, NAME)
    except FileNotFoundError:
        pass


def refresh():
    """Keep an enabled entry pointing at this copy of the app."""
    value = current()
    if value is not None and value != command():
        enable()

"""Windows notification-area icon; sends actions to Tk through a queue.

Uses the Windows APIs directly, so the packaged widget needs no extra dependencies.
The hidden owner window and its message loop live on their own thread.
"""
import ctypes as c
from ctypes import wintypes as w
import threading


class NotifyIconData(c.Structure):
    _fields_ = [
        ("cbSize", w.DWORD), ("hWnd", w.HWND), ("uID", w.UINT),
        ("uFlags", w.UINT), ("uCallbackMessage", w.UINT), ("hIcon", w.HICON),
        ("szTip", w.WCHAR * 128), ("dwState", w.DWORD), ("dwStateMask", w.DWORD),
        ("szInfo", w.WCHAR * 256), ("uVersion", w.UINT),
        ("szInfoTitle", w.WCHAR * 64), ("dwInfoFlags", w.DWORD),
        ("guidItem", c.c_byte * 16), ("hBalloonIcon", w.HICON),
    ]


class WindowsTray:
    CALLBACK = 0x8001
    ACTIONS = ((1, "Show tracker", "show"), (2, "Refresh now", "refresh"),
               (3, "Open dashboard", "dashboard"), (4, "Exit", "quit"))

    def __init__(self, events):
        self.events = events
        self.available = False
        self.hwnd = None
        self.stopping = threading.Event()
        self.thread = threading.Thread(target=self._run, daemon=True, name="tracker-tray")

    def start(self):
        self.thread.start()

    def stop(self):
        self.stopping.set()
        if self.hwnd:
            self.user.PostMessageW(self.hwnd, 0x0010, 0, 0)  # WM_CLOSE
        if self.thread.is_alive():
            self.thread.join(timeout=2)

    def notify(self, title, text):
        """Show a Windows notification from the tray icon; False if there's no icon to show it from."""
        if not self.available or not self.hwnd:
            return False
        # A fresh struct, so this (Tk-thread) call never touches the one the tray thread owns.
        info = NotifyIconData()
        info.cbSize, info.hWnd, info.uID = c.sizeof(info), self.hwnd, 1
        info.uFlags = 0x10  # NIF_INFO
        info.szInfoTitle, info.szInfo = title[:63], text[:255]
        info.dwInfoFlags = 2  # NIIF_WARNING
        return bool(self.shell.Shell_NotifyIconW(1, c.byref(info)))  # NIM_MODIFY

    def _configure_api(self):
        self.user = c.WinDLL("user32", use_last_error=True)
        self.shell = c.WinDLL("shell32", use_last_error=True)
        self.kernel = c.WinDLL("kernel32", use_last_error=True)
        self.callback_type = c.WINFUNCTYPE(c.c_ssize_t, w.HWND, w.UINT, w.WPARAM, w.LPARAM)

        class WindowClass(c.Structure):
            _fields_ = [("style", w.UINT), ("lpfnWndProc", self.callback_type),
                        ("cbClsExtra", c.c_int), ("cbWndExtra", c.c_int),
                        ("hInstance", w.HINSTANCE), ("hIcon", w.HICON),
                        ("hCursor", w.HANDLE), ("hbrBackground", w.HBRUSH),
                        ("lpszMenuName", w.LPCWSTR), ("lpszClassName", w.LPCWSTR)]

        self.window_class_type = WindowClass
        signatures = {
            "RegisterClassW": ([c.POINTER(WindowClass)], w.ATOM),
            "UnregisterClassW": ([w.LPCWSTR, w.HINSTANCE], w.BOOL),
            "CreateWindowExW": ([w.DWORD, w.LPCWSTR, w.LPCWSTR, w.DWORD,
                                 c.c_int, c.c_int, c.c_int, c.c_int,
                                 w.HWND, w.HMENU, w.HINSTANCE, c.c_void_p], w.HWND),
            "DefWindowProcW": ([w.HWND, w.UINT, w.WPARAM, w.LPARAM], c.c_ssize_t),
            "DestroyWindow": ([w.HWND], w.BOOL),
            "PostMessageW": ([w.HWND, w.UINT, w.WPARAM, w.LPARAM], w.BOOL),
            "PostQuitMessage": ([c.c_int], None),
            "GetMessageW": ([c.POINTER(w.MSG), w.HWND, w.UINT, w.UINT], w.BOOL),
            "TranslateMessage": ([c.POINTER(w.MSG)], w.BOOL),
            "DispatchMessageW": ([c.POINTER(w.MSG)], c.c_ssize_t),
            "RegisterWindowMessageW": ([w.LPCWSTR], w.UINT),
            "LoadIconW": ([w.HINSTANCE, c.c_void_p], w.HICON),
            "CreatePopupMenu": ([], w.HMENU),
            "AppendMenuW": ([w.HMENU, w.UINT, c.c_size_t, w.LPCWSTR], w.BOOL),
            "SetMenuDefaultItem": ([w.HMENU, w.UINT, w.UINT], w.BOOL),
            "GetCursorPos": ([c.POINTER(w.POINT)], w.BOOL),
            "SetForegroundWindow": ([w.HWND], w.BOOL),
            "TrackPopupMenu": ([w.HMENU, w.UINT, c.c_int, c.c_int,
                                c.c_int, w.HWND, c.c_void_p], w.UINT),
            "DestroyMenu": ([w.HMENU], w.BOOL),
        }
        for name, (args, result) in signatures.items():
            function = getattr(self.user, name)
            function.argtypes, function.restype = args, result
        self.kernel.GetModuleHandleW.argtypes = [w.LPCWSTR]
        self.kernel.GetModuleHandleW.restype = w.HINSTANCE
        self.shell.Shell_NotifyIconW.argtypes = [w.DWORD, c.POINTER(NotifyIconData)]
        self.shell.Shell_NotifyIconW.restype = w.BOOL

    def _add_icon(self):
        self.available = bool(self.shell.Shell_NotifyIconW(0, c.byref(self.icon)))
        if self.available:
            self.icon.uVersion = 4
            self.shell.Shell_NotifyIconW(4, c.byref(self.icon))  # NIM_SETVERSION
        else:
            self.events.put(("error", "Tray icon unavailable; tracker kept visible"))

    def _menu(self, hwnd):
        menu = self.user.CreatePopupMenu()
        if not menu:
            self.events.put("show")
            return
        try:
            for command, label, _ in self.ACTIONS:
                if command == 4:
                    self.user.AppendMenuW(menu, 0x800, 0, None)  # separator
                self.user.AppendMenuW(menu, 0, command, label)
            self.user.SetMenuDefaultItem(menu, 1, False)
            point = w.POINT()
            self.user.GetCursorPos(c.byref(point))
            self.user.SetForegroundWindow(hwnd)
            command = self.user.TrackPopupMenu(menu, 0x102, point.x, point.y, 0, hwnd, None)
            self.user.PostMessageW(hwnd, 0, 0, 0)
            for number, _, action in self.ACTIONS:
                if command == number:
                    self.events.put(action)
        finally:
            self.user.DestroyMenu(menu)

    def _window_proc(self, hwnd, message, wp, lp):
        try:
            if message == self.CALLBACK:
                event = lp & 0xFFFF
                if event in (0x0400, 0x0401, 0x0203):  # NIN_SELECT, NIN_KEYSELECT, double-click
                    self.events.put("show")
                elif event == 0x007B:  # WM_CONTEXTMENU (mouse or keyboard)
                    self._menu(hwnd)
                return 0
            if message == self.taskbar_created:
                self._add_icon()  # Explorer restarted
                return 0
            if message == 0x0010:  # WM_CLOSE
                self.user.DestroyWindow(hwnd)
                return 0
            if message == 0x0002:  # WM_DESTROY
                self.user.PostQuitMessage(0)
                return 0
        except Exception as error:
            self.events.put(("error", f"Tray error: {error}"))
        return self.user.DefWindowProcW(hwnd, message, wp, lp)

    def _run(self):
        registered = False
        try:
            self._configure_api()
            self.taskbar_created = self.user.RegisterWindowMessageW("TaskbarCreated")
            self.instance = self.kernel.GetModuleHandleW(None)
            self.class_name = f"AIUsageTrackerTray_{id(self)}"
            self.callback = self.callback_type(self._window_proc)
            wc = self.window_class_type()
            wc.lpfnWndProc, wc.hInstance, wc.lpszClassName = self.callback, self.instance, self.class_name
            if not self.user.RegisterClassW(c.byref(wc)):
                raise c.WinError(c.get_last_error())
            registered = True
            self.hwnd = self.user.CreateWindowExW(0, self.class_name, "AI Usage Tracker tray",
                                                  0, 0, 0, 0, 0, None, None, self.instance, None)
            if not self.hwnd:
                raise c.WinError(c.get_last_error())
            self.icon = NotifyIconData()
            self.icon.cbSize, self.icon.hWnd, self.icon.uID = c.sizeof(self.icon), self.hwnd, 1
            self.icon.uFlags = 1 | 2 | 4 | 0x80  # message, icon, tooltip, show tooltip
            self.icon.uCallbackMessage = self.CALLBACK
            self.icon.hIcon = self.user.LoadIconW(None, c.c_void_p(32512))  # shared application icon
            if not self.icon.hIcon:
                raise c.WinError(c.get_last_error())
            self.icon.szTip = "AI Usage Tracker - click to show; right-click for menu"
            self._add_icon()
            if self.stopping.is_set():
                return
            message = w.MSG()
            while True:
                result = self.user.GetMessageW(c.byref(message), None, 0, 0)
                if result == -1:
                    raise c.WinError(c.get_last_error())
                if result == 0:
                    break
                self.user.TranslateMessage(c.byref(message))
                self.user.DispatchMessageW(c.byref(message))
        except Exception as error:
            self.events.put(("error", f"Tray unavailable: {error}"))
        finally:
            self.available = False
            if hasattr(self, "icon"):
                self.shell.Shell_NotifyIconW(2, c.byref(self.icon))  # NIM_DELETE
            if self.hwnd:
                self.user.DestroyWindow(self.hwnd)
                self.hwnd = None
            if registered:
                self.user.UnregisterClassW(self.class_name, self.instance)

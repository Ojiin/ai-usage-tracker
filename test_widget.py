"""Background tracking and tray lifecycle regressions; no user logs or credentials."""
import queue
import unittest
from unittest.mock import Mock, patch

import widget
from tray import WindowsTray


class TrayTests(unittest.TestCase):
    def make_widget(self):
        app = widget.Widget.__new__(widget.Widget)
        app.root = Mock()
        app.status = Mock()
        app.tray = Mock(available=True)
        app.tray_events = queue.Queue()
        app.results = queue.Queue()
        app.scanning = False
        app.settings = {"x": 100, "y": 80}
        return app

    def test_hidden_widget_keeps_refreshing_and_processing_scan_results(self):
        app = self.make_widget()
        app.show = Mock()
        app.hide_to_tray()
        app.root.withdraw.assert_called_once()
        app.root.destroy.assert_not_called()
        with patch.object(widget.threading, 'Thread') as thread:
            app.auto_refresh()
            thread.return_value.start.assert_called_once()
        data = {"test": "scan finished while hidden"}
        app.results.put(("ok", data))
        app.poll()
        app.show.assert_called_once_with(data)
        self.assertFalse(app.scanning)
        app.root.after.assert_any_call(widget.REFRESH_MS, app.auto_refresh)
        app.root.after.assert_any_call(200, app.poll)
        app.root.deiconify.assert_not_called()

    def test_hide_is_refused_without_a_working_tray(self):
        app = self.make_widget()
        app.tray.available = False
        app.hide_to_tray()
        app.root.withdraw.assert_not_called()
        app.status.configure.assert_called_once()

    def test_show_event_restores_window_and_preserves_position(self):
        app = self.make_widget()
        app.tray_events.put("show")
        with patch.object(widget, 'virtual_screen', return_value=(0, 0, 1920, 1080)):
            app.poll()
        app.root.geometry.assert_called_once_with('+100+80')
        app.root.deiconify.assert_called_once()

    def test_tray_failure_restores_hidden_window(self):
        app = self.make_widget()
        app.tray_events.put(("error", "Tray unavailable"))
        with patch.object(widget, 'virtual_screen', return_value=(0, 0, 1920, 1080)):
            app.poll()
        app.root.deiconify.assert_called_once()
        app.status.configure.assert_called_with(text="Tray unavailable")

    def test_exit_stops_tray_and_does_not_reschedule_poll(self):
        app = self.make_widget()
        app.tray_events.put("quit")
        app.poll()
        app.tray.stop.assert_called_once()
        app.root.destroy.assert_called_once()
        app.root.after.assert_not_called()

    def test_tray_mouse_and_keyboard_activation_are_queued_for_tk(self):
        events = queue.Queue()
        tray = WindowsTray(events)
        tray.taskbar_created = 0xC000
        for event in (0x0400, 0x0401, 0x0203):
            tray._window_proc(1, tray.CALLBACK, 0, (1 << 16) | event)
            self.assertEqual(events.get_nowait(), "show")


if __name__ == '__main__':
    unittest.main()

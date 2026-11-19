//! What happens to the window: shown, hidden to the tray, peeking above the tray icon.
//!
//! The rules of widget.py's `hide_to_tray`, `restore`, `peek`, `_track_peek`, `refresh` and
//! `poll`, kept free of any window so they can be tested the way test_widget.py tests the
//! original. Each method changes the state and returns what `lib.rs` must then do to the real
//! window.

use crate::place::Bounds;

/// What the tray icon can ask for (`WindowsTray.events` in tray.py).
#[derive(Debug, Clone, PartialEq)]
pub enum TrayEvent {
    Show,
    Refresh,
    Dashboard,
    Quit,
    /// The pointer is resting on the icon, whose screen rectangle this is.
    Peek(Bounds),
    /// The icon could not be made or was lost.
    Error(String),
}

/// What to do to the real window and app.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Take the window off the screen and the taskbar.
    Withdraw,
    /// Put the window back at its saved position and bring it forward.
    Restore,
    /// Always-on-top goes back to the user's setting (a peek forces it on).
    SettingTopmost,
    /// Show the window above the tray icon, on top of everything.
    PeekAt(Bounds),
    /// Start watching the pointer until it leaves the icon and the widget.
    TrackPeek,
    CloseHoverCard,
    Status(String),
    StartScan,
    OpenDashboard,
    /// Remove the tray icon and end the app.
    Exit,
}

#[derive(Debug, Default)]
pub struct Lifecycle {
    /// The window is withdrawn to the tray.
    pub hidden: bool,
    /// The tray icon the window is peeking above, while it does.
    pub peeking: Option<Bounds>,
    pub tray_available: bool,
    pub scanning: bool,
    pub hover_card_open: bool,
}

impl Lifecycle {
    /// `refresh`: a scan starts unless one is already running.
    pub fn begin_scan(&mut self) -> bool {
        !std::mem::replace(&mut self.scanning, true)
    }

    /// A scan's result arrived (shown or hidden makes no difference).
    pub fn end_scan(&mut self) {
        self.scanning = false;
    }

    fn end_peek(&mut self) -> Vec<Effect> {
        match self.peeking.take() {
            Some(_) => vec![Effect::SettingTopmost],
            None => Vec::new(),
        }
    }

    /// `hide_to_tray`: only with a tray icon to come back from.
    pub fn hide_to_tray(&mut self) -> Vec<Effect> {
        let mut effects = self.end_peek();
        if self.tray_available {
            self.hidden = true;
            effects.push(Effect::Withdraw);
        } else {
            effects.push(Effect::Status(
                "Tray icon unavailable — please try again".into(),
            ));
        }
        effects
    }

    /// `restore`.
    pub fn restore(&mut self) -> Vec<Effect> {
        let mut effects = self.end_peek();
        self.hidden = false;
        effects.push(Effect::Restore);
        effects
    }

    /// `poll`, the tray half: one event from the tray icon.
    pub fn on_tray(&mut self, event: TrayEvent) -> Vec<Effect> {
        match event {
            TrayEvent::Show => self.restore(),
            TrayEvent::Refresh => vec![Effect::StartScan],
            TrayEvent::Dashboard => vec![Effect::OpenDashboard],
            TrayEvent::Quit => vec![Effect::Exit],
            TrayEvent::Peek(icon) => self.peek(icon),
            TrayEvent::Error(message) => {
                // Without an icon a hidden window could never come back.
                self.tray_available = false;
                let mut effects = self.restore();
                effects.push(Effect::Status(message));
                effects
            }
        }
    }

    /// `peek`: show the hidden widget by the tray icon while the pointer rests on it.
    fn peek(&mut self, icon: Bounds) -> Vec<Effect> {
        if !self.hidden && self.peeking.is_none() {
            return Vec::new(); // already up the normal way
        }
        let first = self.peeking.replace(icon).is_none();
        self.hidden = false;
        let mut effects = vec![Effect::PeekAt(icon)];
        if first {
            effects.push(Effect::TrackPeek);
        }
        effects
    }

    /// `_track_peek`, once it found the pointer somewhere else: back to the tray.
    pub fn peek_over(&mut self) -> Vec<Effect> {
        let mut effects = self.end_peek();
        if effects.is_empty() {
            return effects; // the peek was ended some other way meanwhile
        }
        self.hidden = true;
        effects.extend([Effect::CloseHoverCard, Effect::Withdraw]);
        effects
    }
}

#[cfg(test)]
mod tests {
    //! test_widget.py, on the state machine instead of a mocked Tk root.
    use super::*;

    const ICON: Bounds = (1800, 1048, 1824, 1072);

    fn app() -> Lifecycle {
        Lifecycle {
            tray_available: true,
            ..Lifecycle::default()
        }
    }

    #[test]
    fn hidden_widget_keeps_refreshing_and_processing_scan_results() {
        let mut app = app();
        assert_eq!(app.hide_to_tray(), [Effect::Withdraw]);
        assert!(app.hidden);
        // the 5-minute timer still starts scans, one at a time
        assert!(app.begin_scan());
        assert!(!app.begin_scan());
        // the result is taken while hidden, and nothing shows the window
        app.end_scan();
        assert!(!app.scanning);
        assert!(app.hidden);
        assert!(app.begin_scan());
        assert_eq!(app.on_tray(TrayEvent::Refresh), [Effect::StartScan]);
        assert!(app.hidden);
    }

    #[test]
    fn hide_is_refused_without_a_working_tray() {
        let mut app = Lifecycle::default();
        assert_eq!(
            app.hide_to_tray(),
            [Effect::Status(
                "Tray icon unavailable — please try again".into()
            )]
        );
        assert!(!app.hidden);
    }

    #[test]
    fn show_event_restores_window() {
        let mut app = app();
        app.hide_to_tray();
        assert_eq!(app.on_tray(TrayEvent::Show), [Effect::Restore]);
        assert!(!app.hidden);
    }

    #[test]
    fn tray_failure_restores_hidden_window() {
        let mut app = app();
        app.hide_to_tray();
        assert_eq!(
            app.on_tray(TrayEvent::Error("Tray unavailable".into())),
            [Effect::Restore, Effect::Status("Tray unavailable".into())]
        );
        assert!(!app.hidden);
        // and it cannot be hidden again until there is an icon
        assert!(matches!(app.hide_to_tray()[..], [Effect::Status(_)]));
    }

    #[test]
    fn exit_and_dashboard_events() {
        let mut app = app();
        assert_eq!(app.on_tray(TrayEvent::Quit), [Effect::Exit]);
        assert_eq!(app.on_tray(TrayEvent::Dashboard), [Effect::OpenDashboard]);
    }

    #[test]
    fn peek_only_when_hidden_and_tracks_once() {
        let mut app = app();
        // visible the normal way: resting on the icon does nothing
        assert_eq!(app.on_tray(TrayEvent::Peek(ICON)), []);
        app.hide_to_tray();
        assert_eq!(
            app.on_tray(TrayEvent::Peek(ICON)),
            [Effect::PeekAt(ICON), Effect::TrackPeek]
        );
        // the shell reports the icon again while it is up: move, but one tracker only
        let moved = (1700, 1048, 1724, 1072);
        assert_eq!(
            app.on_tray(TrayEvent::Peek(moved)),
            [Effect::PeekAt(moved)]
        );
        assert_eq!(app.peeking, Some(moved));
        // pointer left: back to the tray, always-on-top back to the setting
        assert_eq!(
            app.peek_over(),
            [
                Effect::SettingTopmost,
                Effect::CloseHoverCard,
                Effect::Withdraw
            ]
        );
        assert!(app.hidden && app.peeking.is_none());
        assert_eq!(app.peek_over(), []);
    }

    #[test]
    fn clicking_the_icon_during_a_peek_keeps_the_window() {
        let mut app = app();
        app.hide_to_tray();
        app.on_tray(TrayEvent::Peek(ICON));
        assert_eq!(
            app.on_tray(TrayEvent::Show),
            [Effect::SettingTopmost, Effect::Restore]
        );
        assert!(!app.hidden);
        assert_eq!(app.peek_over(), []); // the tracker finds the peek ended and stops
    }

    #[test]
    fn hiding_during_a_peek_ends_it() {
        let mut app = app();
        app.hide_to_tray();
        app.on_tray(TrayEvent::Peek(ICON));
        assert_eq!(
            app.hide_to_tray(),
            [Effect::SettingTopmost, Effect::Withdraw]
        );
        assert!(app.hidden && app.peeking.is_none());
    }
}

//! The notification-area icon (tray.py, on Tauri's tray API instead of raw Win32 calls).
//!
//! Left-click or double-click shows the tracker, resting the pointer on the icon lets the
//! widget peek above it, and right-click opens Show tracker / Refresh now / Open dashboard /
//! Exit. If the icon cannot be created `build` fails and the widget stays on screen.

use crate::lifecycle::TrayEvent;
use crate::place::Bounds;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

pub const TOOLTIP: &str = "AI Usage Tracker - hover to peek, click to show, right-click for menu";

/// The tray menu's entries: menu id, label, what it asks for (`WindowsTray.ACTIONS`).
pub fn actions() -> [(&'static str, &'static str, TrayEvent); 4] {
    [
        ("tray-show", "Show tracker", TrayEvent::Show),
        ("tray-refresh", "Refresh now", TrayEvent::Refresh),
        ("tray-dashboard", "Open dashboard", TrayEvent::Dashboard),
        ("tray-exit", "Exit", TrayEvent::Quit),
    ]
}

/// What a click in the tray menu asks for.
pub fn menu_event(id: &str) -> Option<TrayEvent> {
    actions()
        .into_iter()
        .find(|(action, _, _)| *action == id)
        .map(|(_, _, event)| event)
}

/// What the pointer did to the icon.
#[derive(Debug, PartialEq)]
pub enum Pointer {
    /// Released the left button, or double-clicked: show the tracker.
    Activate,
    /// Came to the icon, whose screen rectangle this is: peek if it stays.
    Enter(Bounds),
}

pub fn pointer_event(event: &TrayIconEvent) -> Option<Pointer> {
    match event {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        }
        | TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        } => Some(Pointer::Activate),
        TrayIconEvent::Enter { rect, .. } => {
            // Windows reports the icon in physical pixels already.
            let at = rect.position.to_physical::<f64>(1.0);
            let size = rect.size.to_physical::<f64>(1.0);
            let (left, top) = (at.x.round() as i64, at.y.round() as i64);
            let (width, height) = (size.width.round() as i64, size.height.round() as i64);
            Some(Pointer::Enter((left, top, left + width, top + height)))
        }
        _ => None,
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let [show, refresh, dashboard, exit] =
        actions().map(|(id, label, _)| MenuItem::with_id(app, id, label, true, None::<&str>));
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show?, &refresh?, &dashboard?, &separator, &exit?])?;
    TrayIconBuilder::with_id("ai-usage-tracker")
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .tooltip(TOOLTIP)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| match pointer_event(&event) {
            Some(Pointer::Activate) => crate::on_tray(tray.app_handle(), TrayEvent::Show),
            Some(Pointer::Enter(icon)) => crate::tray_rest(tray.app_handle(), icon),
            None => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::tray::TrayIconId;
    use tauri::{PhysicalPosition, PhysicalSize, Rect};

    fn rect() -> Rect {
        Rect {
            position: PhysicalPosition::new(1800, 1048).into(),
            size: PhysicalSize::new(24, 24).into(),
        }
    }

    // test_widget.py: test_tray_mouse_and_keyboard_activation_are_queued_for_tk
    #[test]
    fn click_and_double_click_show_the_tracker() {
        let (id, position) = (TrayIconId::new("t"), PhysicalPosition::new(1810.0, 1060.0));
        let click = |button, button_state| TrayIconEvent::Click {
            id: id.clone(),
            position,
            rect: rect(),
            button,
            button_state,
        };
        assert_eq!(
            pointer_event(&click(MouseButton::Left, MouseButtonState::Up)),
            Some(Pointer::Activate)
        );
        assert_eq!(
            pointer_event(&click(MouseButton::Left, MouseButtonState::Down)),
            None
        );
        assert_eq!(
            pointer_event(&click(MouseButton::Right, MouseButtonState::Up)),
            None
        );
        let double = TrayIconEvent::DoubleClick {
            id: id.clone(),
            position,
            rect: rect(),
            button: MouseButton::Left,
        };
        assert_eq!(pointer_event(&double), Some(Pointer::Activate));
    }

    #[test]
    fn resting_on_the_icon_reports_its_rectangle() {
        let enter = TrayIconEvent::Enter {
            id: TrayIconId::new("t"),
            position: PhysicalPosition::new(1810.0, 1060.0),
            rect: rect(),
        };
        assert_eq!(
            pointer_event(&enter),
            Some(Pointer::Enter((1800, 1048, 1824, 1072)))
        );
        let leave = TrayIconEvent::Leave {
            id: TrayIconId::new("t"),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: rect(),
        };
        assert_eq!(pointer_event(&leave), None);
    }

    #[test]
    fn menu_matches_the_python_tray() {
        let labels: Vec<&str> = actions().iter().map(|(_, label, _)| *label).collect();
        assert_eq!(
            labels,
            ["Show tracker", "Refresh now", "Open dashboard", "Exit"]
        );
        assert_eq!(menu_event("tray-show"), Some(TrayEvent::Show));
        assert_eq!(menu_event("tray-refresh"), Some(TrayEvent::Refresh));
        assert_eq!(menu_event("tray-dashboard"), Some(TrayEvent::Dashboard));
        assert_eq!(menu_event("tray-exit"), Some(TrayEvent::Quit));
        assert_eq!(menu_event("exit"), None);
        assert!(TOOLTIP.encode_utf16().count() < 128); // the shell's limit for a tooltip
    }
}

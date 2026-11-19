//! Where windows go: the start position, the peek above the tray icon, the hover card.
//!
//! All in physical pixels, as widget.py worked (it asked Windows for the same units), so a
//! position saved by the Python widget lands in the same place.

/// A rectangle as (left, top, right, bottom); right and bottom are just outside it.
pub type Bounds = (i64, i64, i64, i64);

/// Pixels between the tray icon and the widget when it peeks from the tray.
pub const PEEK_GAP: i64 = 8;
/// Pixels between the "● N working" label and its hover card.
pub const TIP_GAP: i64 = 4;

/// `virtual_screen()`: the box around all monitors, each given as (x, y, width, height).
pub fn virtual_screen(monitors: &[(i64, i64, i64, i64)]) -> Option<Bounds> {
    monitors
        .iter()
        .map(|&(x, y, w, h)| (x, y, x + w, y + h))
        .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
}

/// The default spot, top-right of the primary screen: 300 px in from its right edge and 60 px
/// down, as in widget.py, but counted in the screen's own scale so the card still fits on a
/// display set to 150%.
pub fn top_right(primary: (i64, i64, i64, i64), scale: f64) -> (i64, i64) {
    let (x, y, width, _) = primary;
    let scaled = |pixels: f64| (pixels * scale).round() as i64;
    (x + width - scaled(300.0), y + scaled(60.0))
}

/// `_start_position`: the saved position, unless there is none or it is off every screen
/// (a monitor was unplugged); then `fallback`.
pub fn start_position(
    saved: (Option<i64>, Option<i64>),
    screen: Option<Bounds>,
    fallback: (i64, i64),
) -> (i64, i64) {
    let (Some(x), Some(y)) = saved else {
        return fallback;
    };
    match screen {
        Some((left, top, right, bottom))
            if !(left <= x && x < right - 40 && top <= y && y < bottom - 40) =>
        {
            fallback
        }
        _ => (x, y),
    }
}

/// `peek`: centred above the tray icon, or below it when the taskbar is along the top.
pub fn peek_position(icon: Bounds, size: (i64, i64), screen: Bounds) -> (i64, i64) {
    let (width, height) = size;
    let (left, top, right, _) = screen;
    let x = (icon.0 + icon.2 - width)
        .div_euclid(2)
        .max(left)
        .min(right - width);
    let above = icon.1 - height - PEEK_GAP;
    (x, if above < top { icon.3 + PEEK_GAP } else { above })
}

pub fn contains(bounds: Bounds, point: (i64, i64)) -> bool {
    bounds.0 <= point.0 && point.0 < bounds.2 && bounds.1 <= point.1 && point.1 < bounds.3
}

/// `_track_peek`: the peek stays up while the pointer is on the icon, on the widget, or
/// crossing the gap between them (the box around both), or on an open hover card.
pub fn peek_stays(pointer: (i64, i64), icon: Bounds, window: Bounds, over_card: bool) -> bool {
    let around = (
        icon.0.min(window.0),
        icon.1.min(window.1),
        icon.2.max(window.2),
        icon.3.max(window.3),
    );
    contains(around, pointer) || over_card
}

/// `_show_working`: the hover card opens under the label, or above it when there is no room
/// below, and never leaves the screen to the left, top or right.
pub fn tip_position(label: Bounds, tip: (i64, i64), screen: Bounds) -> (i64, i64) {
    let (left, top, right, bottom) = screen;
    let x = label.0.min(right - tip.0);
    let mut y = label.3 + TIP_GAP;
    if y + tip.1 > bottom {
        y = label.1 - tip.1 - TIP_GAP;
    }
    (x.max(left), y.max(top))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_HD: Bounds = (0, 0, 1920, 1080);

    #[test]
    fn saved_position_is_kept_while_it_is_on_a_screen() {
        let fallback = (1620, 60);
        let at = |x, y, screen| start_position((x, y), screen, fallback);
        assert_eq!(at(Some(100), Some(80), Some(FULL_HD)), (100, 80));
        assert_eq!(at(Some(0), Some(0), Some(FULL_HD)), (0, 0));
        assert_eq!(at(Some(1879), Some(1039), Some(FULL_HD)), (1879, 1039));
        // within 40 px of the far edges counts as off-screen
        assert_eq!(at(Some(1880), Some(80), Some(FULL_HD)), fallback);
        assert_eq!(at(Some(100), Some(1040), Some(FULL_HD)), fallback);
        assert_eq!(at(Some(-1), Some(80), Some(FULL_HD)), fallback);
        // a second monitor to the left makes negative positions fine
        assert_eq!(
            at(Some(-1500), Some(200), Some((-1920, 0, 1920, 1080))),
            (-1500, 200)
        );
        // that monitor unplugged
        assert_eq!(at(Some(-1500), Some(200), Some(FULL_HD)), fallback);
        // never moved, or half a position
        assert_eq!(at(None, None, Some(FULL_HD)), fallback);
        assert_eq!(at(Some(100), None, Some(FULL_HD)), fallback);
        // screen size unknown: trust the saved position
        assert_eq!(at(Some(5000), Some(80), None), (5000, 80));
    }

    #[test]
    fn default_is_top_right_of_the_primary_screen() {
        assert_eq!(top_right((0, 0, 1920, 1080), 1.0), (1620, 60));
        assert_eq!(top_right((0, 0, 2880, 1800), 1.5), (2430, 90));
    }

    #[test]
    fn virtual_screen_spans_all_monitors() {
        assert_eq!(virtual_screen(&[]), None);
        assert_eq!(virtual_screen(&[(0, 0, 1920, 1080)]), Some(FULL_HD));
        assert_eq!(
            virtual_screen(&[(0, 0, 1920, 1080), (-1280, 56, 1280, 1024), (1920, -200, 2560, 1440)]),
            Some((-1280, -200, 4480, 1240))
        );
    }

    #[test]
    fn peek_sits_above_the_icon_or_below_a_top_taskbar() {
        let size = (290, 600);
        // icon near the clock on a bottom taskbar: clamped to the right edge
        assert_eq!(
            peek_position((1800, 1048, 1824, 1072), size, FULL_HD),
            (1630, 440)
        );
        // room to centre
        assert_eq!(
            peek_position((1000, 1048, 1024, 1072), size, FULL_HD),
            (867, 440)
        );
        // taskbar along the top
        assert_eq!(peek_position((1800, 8, 1824, 32), size, FULL_HD), (1630, 40));
        // left taskbar: clamped to the left edge; odd sums round down as Python's // does
        assert_eq!(peek_position((8, 500, 31, 524), (290, 400), FULL_HD), (0, 92));
        assert_eq!(peek_position((301, 900, 302, 924), (290, 400), FULL_HD), (156, 492));
    }

    #[test]
    fn peek_stays_while_the_pointer_is_between_icon_and_widget() {
        let icon = (1800, 1048, 1824, 1072);
        let window = (1630, 440, 1920, 1040);
        assert!(peek_stays((1810, 1060), icon, window, false)); // on the icon
        assert!(peek_stays((1700, 700), icon, window, false)); // on the widget
        assert!(peek_stays((1810, 1044), icon, window, false)); // crossing the gap
        assert!(!peek_stays((1500, 700), icon, window, false)); // left of both
        assert!(!peek_stays((1810, 1072), icon, window, false)); // just below the icon
        assert!(peek_stays((1500, 700), icon, window, true)); // on the hover card
    }

    #[test]
    fn hover_card_opens_below_or_above_and_stays_on_screen() {
        let label = (1700, 70, 1760, 84);
        assert_eq!(tip_position(label, (400, 150), FULL_HD), (1520, 88));
        assert_eq!(tip_position((100, 70, 160, 84), (400, 150), FULL_HD), (100, 88));
        let low = (100, 1000, 160, 1014);
        assert_eq!(tip_position(low, (400, 150), FULL_HD), (100, 846));
        assert_eq!(tip_position((-50, 5, 10, 19), (400, 2000), FULL_HD), (0, 0));
    }
}

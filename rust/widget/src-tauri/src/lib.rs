//! AI Usage Tracker widget: a small always-on-top card you can drag anywhere.
//!
//! The Tauri port of widget.py, tray.py and startup.py. Who does what:
//!
//! - `core.rs`: the only door to the ported core crates (scan, dashboard, working sessions,
//!   limit alerts, API balance). Also the fake-data mode used to develop the page.
//! - `view.rs` and `format.rs`: turn one scan into the view model, every text already worded
//!   exactly as the Python widget worded it.
//! - `lifecycle.rs` and `place.rs`: the rules for hiding to the tray, peeking, and where
//!   windows go, free of any real window so they can be tested.
//! - `settings.rs`, `autostart.rs`, `tray.rs`: widget.json, the Run-key entry, the tray icon.
//! - this file: the windows, timers and threads that connect them.
//!
//! The page in `ui/` only draws the view model and sends clicks back as commands.
//!
//! One rule for this file: never touch a window while holding the state lock (`with`). Window
//! calls wait for the main thread, and the main thread may be waiting for the lock.

mod autostart;
mod core;
pub mod format;
mod lifecycle;
mod place;
mod settings;
mod tray;
pub mod view;

use lifecycle::{Effect, Lifecycle, TrayEvent};
use place::Bounds;
use serde::Serialize;
use serde_json::Value;
use settings::Settings;
use std::sync::{mpsc, Mutex, PoisonError};
use std::time::Duration;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent, Wry,
};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use view::{Clock, Status, TipRow, View, MUTED, WARN};

/// A rescan every 5 minutes.
const REFRESH: Duration = Duration::from_secs(5 * 60);
/// Reset countdowns and the working count tick between scans.
const COUNTDOWN: Duration = Duration::from_secs(30);
/// How often an open hover card or a peek checks where the pointer is.
const HOVER_POLL: Duration = Duration::from_millis(150);
/// How long the pointer must rest on the tray icon before the widget peeks.
const PEEK_REST: Duration = Duration::from_millis(400);
/// A drag is saved once the window has stopped moving for this long.
const DRAG_SETTLE: Duration = Duration::from_millis(400);

/// Whether the user may resize the windows: no. Except on Linux, where the widget is only
/// developed: GTK will not let the app shrink a fixed-size window to its content either.
const USER_RESIZABLE: bool = cfg!(target_os = "linux");

/// Everything that changes while the app runs.
struct Shared {
    settings: Settings,
    life: Lifecycle,
    last_data: Option<Value>,
    status: Status,
    /// "● 2 working", or empty.
    working: String,
    /// The "● N working" label on the screen, while its hover card is wanted.
    label: Option<Bounds>,
    /// The user is dragging the window, so its moves are theirs and get saved.
    dragging: bool,
    moves: Option<mpsc::Sender<(i32, i32)>>,
}

/// The right-click menu, kept so its ticks can follow the settings.
struct Menus {
    context: Menu<Wry>,
    topmost: CheckMenuItem<Wry>,
    alerts: CheckMenuItem<Wry>,
    autostart: CheckMenuItem<Wry>,
}

/// What the page draws.
#[derive(Serialize, Clone)]
struct Screen {
    view: View,
    working: String,
    status: Status,
}

fn with<R>(app: &AppHandle, f: impl FnOnce(&mut Shared) -> R) -> R {
    let state = app.state::<Mutex<Shared>>();
    let mut shared = state.lock().unwrap_or_else(PoisonError::into_inner);
    f(&mut shared)
}

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

fn current_screen(app: &AppHandle) -> Screen {
    let clock = Clock::system();
    with(app, |s| Screen {
        view: s
            .last_data
            .as_ref()
            .map_or_else(View::empty, |data| View::build(data, &clock)),
        working: s.working.clone(),
        status: s.status.clone(),
    })
}

/// Send the page what to draw now.
fn push(app: &AppHandle) {
    let _ = app.emit_to("main", "screen", current_screen(app));
}

fn set_status(app: &AppHandle, text: &str, color: Option<&'static str>) {
    with(app, |s| {
        s.status.text = text.to_string();
        s.status.color = color.unwrap_or(s.status.color);
    });
    push(app);
}

// --- places -------------------------------------------------------------------

fn bounds_of(window: &WebviewWindow) -> Option<Bounds> {
    let (at, size) = (window.outer_position().ok()?, window.outer_size().ok()?);
    let (left, top) = (i64::from(at.x), i64::from(at.y));
    Some((
        left,
        top,
        left + i64::from(size.width),
        top + i64::from(size.height),
    ))
}

/// The box around all monitors, or `None` when the system will not say.
fn virtual_screen(window: &WebviewWindow) -> Option<Bounds> {
    let monitors: Vec<_> = window
        .available_monitors()
        .ok()?
        .iter()
        .map(|m| {
            let (at, size) = (m.position(), m.size());
            (
                i64::from(at.x),
                i64::from(at.y),
                i64::from(size.width),
                i64::from(size.height),
            )
        })
        .collect();
    place::virtual_screen(&monitors)
}

fn primary_screen(window: &WebviewWindow) -> Option<(i64, i64, i64, i64, f64)> {
    let monitor = window.primary_monitor().ok().flatten()?;
    let (at, size) = (monitor.position(), monitor.size());
    Some((
        i64::from(at.x),
        i64::from(at.y),
        i64::from(size.width),
        i64::from(size.height),
        monitor.scale_factor(),
    ))
}

/// All monitors, else the primary one, else something that keeps the arithmetic harmless.
fn whole_screen(window: &WebviewWindow) -> Bounds {
    virtual_screen(window)
        .or_else(|| primary_screen(window).map(|(x, y, w, h, _)| (x, y, x + w, y + h)))
        .unwrap_or((0, 0, 1920, 1080))
}

fn start_position(app: &AppHandle, window: &WebviewWindow) -> (i64, i64) {
    let saved = with(app, |s| s.settings.position());
    let fallback = primary_screen(window).map_or((60, 60), |(x, y, w, h, scale)| {
        place::top_right((x, y, w, h), scale)
    });
    place::start_position(saved, virtual_screen(window), fallback)
}

/// Move the window ourselves; such a move is not a drag and is not saved.
fn move_to(app: &AppHandle, window: &WebviewWindow, at: (i64, i64)) {
    with(app, |s| s.dragging = false);
    let _ = window.set_position(PhysicalPosition::new(at.0 as i32, at.1 as i32));
}

fn pointer(app: &AppHandle) -> Option<(i64, i64)> {
    let at = app.cursor_position().ok()?;
    Some((at.x.floor() as i64, at.y.floor() as i64))
}

/// Saves the position a drag ended at. One thread for the app's life; it writes once the
/// window has stopped moving, not on every step of the drag.
fn save_drags(app: AppHandle) -> mpsc::Sender<(i32, i32)> {
    let (sender, moves) = mpsc::channel::<(i32, i32)>();
    std::thread::spawn(move || {
        while let Ok(mut at) = moves.recv() {
            while let Ok(next) = moves.recv_timeout(DRAG_SETTLE) {
                at = next;
            }
            with(&app, |s| {
                s.settings.set_position(at.0.into(), at.1.into());
                s.settings.save();
            });
        }
    });
    sender
}

// --- data ---------------------------------------------------------------------

/// Start a scan on its own thread, unless one is running. The window stays responsive, and
/// a window hidden to the tray keeps being refreshed.
fn refresh(app: &AppHandle) {
    if !with(app, |s| s.life.begin_scan()) {
        return;
    }
    set_status(app, "Scanning…", None);
    let app = app.clone();
    std::thread::spawn(move || {
        let scan = core::collect(&mut |_| {}).and_then(|data| {
            core::write_dashboard(&data, &core::dashboard_path())?;
            Ok(data)
        });
        finish_scan(&app, scan);
    });
}

fn finish_scan(app: &AppHandle, scan: Result<Value, String>) {
    let clock = Clock::system();
    // State advances even with alerts off, so turning them on doesn't replay old crossings.
    let alerts = scan.as_ref().map_or_else(
        |_| Vec::new(),
        |data| {
            let now = data.get("now_epoch").and_then(Value::as_f64);
            core::due_alerts(
                data.get("plan_limits").unwrap_or(&Value::Null),
                now.unwrap_or(clock.now),
                &|t| clock.at(t as f64, "%H:%M"),
            )
        },
    );
    let notify = with(app, |s| {
        s.life.end_scan();
        match scan {
            Ok(data) => {
                s.status = view::updated(&data, &clock);
                s.last_data = Some(data);
                if !s.settings.flag("alerts", true) {
                    return Vec::new();
                }
                // Focus Assist can swallow the toast; the status line keeps the latest one visible.
                if let Some((title, _)) = alerts.last() {
                    s.status = Status {
                        text: format!("⚠ {title}"),
                        color: WARN,
                    };
                }
                alerts
            }
            Err(error) => {
                s.status.text = view::scan_failed(&error);
                Vec::new()
            }
        }
    });
    for (title, text) in notify {
        let _ = app.notification().builder().title(title).body(text).show();
    }
    push(app);
}

/// Every 30 seconds: the working count, and the page again so the countdowns move.
fn tick(app: &AppHandle) {
    let working =
        core::working_now().map_or_else(|_| String::new(), |active| view::working(&active));
    with(app, |s| s.working = working);
    push(app);
}

// --- window lifecycle ---------------------------------------------------------

fn apply(app: &AppHandle, effects: Vec<Effect>) {
    let Some(window) = main_window(app) else {
        return;
    };
    for effect in effects {
        match effect {
            Effect::Withdraw => {
                let _ = window.hide();
            }
            Effect::Restore => {
                move_to(app, &window, start_position(app, &window));
                let _ = window.show();
                let _ = window.set_focus();
            }
            Effect::SettingTopmost => {
                let _ = window.set_always_on_top(with(app, |s| s.settings.flag("topmost", true)));
            }
            Effect::PeekAt(icon) => {
                let size = window.outer_size().map_or((0, 0), |size| {
                    (i64::from(size.width), i64::from(size.height))
                });
                let at = place::peek_position(icon, size, whole_screen(&window));
                move_to(app, &window, at);
                let _ = window.set_always_on_top(true);
                let _ = window.show();
            }
            Effect::TrackPeek => track_peek(app.clone()),
            Effect::CloseHoverCard => close_hover_card(app),
            Effect::Status(text) => set_status(app, &text, None),
            Effect::StartScan => refresh(app),
            Effect::OpenDashboard => open_dashboard(app),
            Effect::Exit => app.exit(0),
        }
    }
}

/// Something the tray icon asked for (also called from tray.rs).
pub(crate) fn on_tray(app: &AppHandle, event: TrayEvent) {
    let effects = with(app, |s| s.life.on_tray(event));
    apply(app, effects);
}

/// The pointer came to the tray icon: peek if it is still there in a moment. (The Python
/// widget got this "resting" signal from the shell; Tauri reports the pointer arriving.)
pub(crate) fn tray_rest(app: &AppHandle, icon: Bounds) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(PEEK_REST);
        if pointer(&app).is_some_and(|at| place::contains(icon, at)) {
            on_tray(&app, TrayEvent::Peek(icon));
        }
    });
}

/// A peek stays up while the pointer is on the icon, on the widget, or crossing the gap between
/// them, so you can move up into it (e.g. to hover "● N working"); it hides once the pointer
/// is anywhere else.
fn track_peek(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(HOVER_POLL);
        let (Some(icon), card_open) = with(&app, |s| (s.life.peeking, s.life.hover_card_open))
        else {
            break; // shown or hidden some other way meanwhile
        };
        let stays = match (pointer(&app), main_window(&app).and_then(|w| bounds_of(&w))) {
            (Some(at), Some(window)) => {
                let on_ours = |label: &str| {
                    let ours = app.get_webview_window(label).and_then(|w| bounds_of(&w));
                    ours.is_some_and(|bounds| place::contains(bounds, at))
                };
                let over_card = card_open && (on_ours("main") || on_ours("tip"));
                place::peek_stays(at, icon, window, over_card)
            }
            _ => false,
        };
        if !stays {
            let effects = with(&app, |s| s.life.peek_over());
            apply(&app, effects);
            break;
        }
    });
}

fn hide_to_tray(app: &AppHandle) {
    let effects = with(app, |s| s.life.hide_to_tray());
    apply(app, effects);
}

fn open_dashboard(app: &AppHandle) {
    let dashboard = core::dashboard_path();
    if dashboard.exists() {
        let _ = app
            .opener()
            .open_path(dashboard.to_string_lossy(), None::<&str>);
    } else {
        set_status(app, "Dashboard not built yet — wait for the scan", None);
    }
}

// --- hover card -----------------------------------------------------------------

fn close_hover_card(app: &AppHandle) {
    with(app, |s| {
        s.life.hover_card_open = false;
        s.label = None;
    });
    if let Some(tip) = app.get_webview_window("tip") {
        let _ = tip.hide();
    }
}

/// The card closes only once the pointer is really off the label.
fn track_hover(app: AppHandle, label: Bounds) {
    std::thread::spawn(move || loop {
        std::thread::sleep(HOVER_POLL);
        if with(&app, |s| s.label) != Some(label) {
            break; // closed, or reopened and tracked by a newer thread
        }
        if !pointer(&app).is_some_and(|at| place::contains(label, at)) {
            close_hover_card(&app);
            break;
        }
    });
}

// --- right-click menu -----------------------------------------------------------

fn context_menu(
    app: &AppHandle,
    topmost: bool,
    alerts: bool,
    autostart: bool,
) -> tauri::Result<Menus> {
    let item = |id, label| MenuItem::with_id(app, id, label, true, None::<&str>);
    let check = |id, label, on| CheckMenuItem::with_id(app, id, label, true, on, None::<&str>);
    let topmost = check("topmost", "Always on top", topmost)?;
    let alerts = check("alerts", "Limit alerts", alerts)?;
    let autostart = check("autostart", "Start with Windows", autostart)?;
    let context = Menu::with_items(
        app,
        &[
            &item("hide", "Hide to tray (keep tracking)")?,
            &item("refresh", "Refresh now")?,
            &item("dashboard", "Open dashboard")?,
            &item("balance", "Set API balance…")?,
            &topmost,
            &alerts,
            &autostart,
            &PredefinedMenuItem::separator(app)?,
            &item("exit", "Exit")?,
        ],
    )?;
    Ok(Menus {
        context,
        topmost,
        alerts,
        autostart,
    })
}

fn on_menu(app: &AppHandle, id: &str) {
    if let Some(event) = tray::menu_event(id) {
        return on_tray(app, event);
    }
    match id {
        "hide" => hide_to_tray(app),
        "refresh" => refresh(app),
        "dashboard" => open_dashboard(app),
        "balance" => open_balance(app),
        "topmost" => toggle_setting(app, "topmost"),
        "alerts" => toggle_setting(app, "alerts"),
        "autostart" => toggle_autostart(app),
        "exit" => app.exit(0),
        _ => {}
    }
}

/// "Always on top" and "Limit alerts": flip, save, and make the tick show the result.
fn toggle_setting(app: &AppHandle, key: &str) {
    let on = with(app, |s| {
        let on = !s.settings.flag(key, true);
        s.settings.set_flag(key, on);
        s.settings.save();
        on
    });
    let menus = app.state::<Menus>();
    if key == "topmost" {
        if let Some(window) = main_window(app) {
            let _ = window.set_always_on_top(on);
        }
        let _ = menus.topmost.set_checked(on);
    } else {
        let _ = menus.alerts.set_checked(on);
    }
}

fn toggle_autostart(app: &AppHandle) {
    let mut entries = autostart::system();
    let on = !autostart::is_enabled(&entries);
    let changed = if on {
        std::env::current_exe().and_then(|exe| autostart::enable(&mut entries, &exe))
    } else {
        autostart::disable(&mut entries)
    };
    match changed {
        Ok(()) if on => set_status(app, "Will start with Windows", Some(MUTED)),
        Ok(()) => set_status(app, "Won't start with Windows", Some(MUTED)),
        Err(error) => set_status(app, &autostart::failure(&error), Some(WARN)),
    }
    // show what Windows actually has
    let _ = app
        .state::<Menus>()
        .autostart
        .set_checked(autostart::is_enabled(&entries));
}

// --- "Set API balance…" ---------------------------------------------------------

fn open_balance(app: &AppHandle) {
    let (Some(main), Some(dialog)) = (main_window(app), app.get_webview_window("balance")) else {
        return;
    };
    if let Ok(at) = main.outer_position() {
        let _ = dialog.set_position(PhysicalPosition::new(at.x, at.y + 40));
    }
    let _ = app.emit_to("balance", "balance-open", core::providers());
    let _ = dialog.show();
    let _ = dialog.set_focus();
}

fn close_balance(app: &AppHandle) {
    if let Some(dialog) = app.get_webview_window("balance") {
        let _ = dialog.hide();
    }
}

// --- commands the pages call ----------------------------------------------------

/// The page has loaded and asks what to draw.
#[tauri::command]
fn screen(app: AppHandle) -> Screen {
    current_screen(&app)
}

/// Left button went down on the card: the window follows the pointer until it is released.
#[tauri::command]
fn drag(app: AppHandle, window: WebviewWindow) {
    with(&app, |s| s.dragging = true);
    let _ = window.start_dragging();
}

/// One of ⟳ ↗ − ✕.
#[tauri::command]
fn press(app: AppHandle, button: String) {
    match button.as_str() {
        "refresh" => refresh(&app),
        "dashboard" => open_dashboard(&app),
        "hide" => hide_to_tray(&app),
        "close" => app.exit(0),
        _ => {}
    }
}

/// Right-click anywhere on the card.
#[tauri::command]
fn menu(app: AppHandle, window: WebviewWindow) {
    let _ = window.popup_menu(&app.state::<Menus>().context);
}

/// A page tells how big its content is; its window takes that size.
#[tauri::command]
fn fit(window: WebviewWindow, width: f64, height: f64) {
    if width >= 1.0 && height >= 1.0 {
        let _ = window.set_size(LogicalSize::new(width.ceil(), height.ceil()));
    }
}

/// The pointer came onto "● N working"; the numbers are where the label is in the page.
/// Asks the hover-card page to lay out one row per working session.
#[tauri::command(async)]
fn working_enter(app: AppHandle, left: f64, top: f64, right: f64, bottom: f64) {
    if with(&app, |s| s.life.hover_card_open) {
        return; // already open
    }
    let sessions = core::working_details().unwrap_or_default();
    let rows: Vec<TipRow> = view::tip(&sessions, &core::describe_session);
    let Some(main) = main_window(&app).filter(|_| !rows.is_empty()) else {
        return;
    };
    let (Ok(origin), Ok(scale)) = (main.inner_position(), main.scale_factor()) else {
        return;
    };
    let on_screen = |page: f64, origin: i32| i64::from(origin) + (page * scale).round() as i64;
    let label = (
        on_screen(left, origin.x),
        on_screen(top, origin.y),
        on_screen(right, origin.x),
        on_screen(bottom, origin.y),
    );
    with(&app, |s| {
        s.life.hover_card_open = true;
        s.label = Some(label);
    });
    let _ = app.emit_to("tip", "tip", rows);
}

/// The hover-card page has laid out its rows and tells their size: place the card under the
/// label (or above it), show it, and watch the pointer.
#[tauri::command(async)]
fn tip_ready(app: AppHandle, window: WebviewWindow, width: f64, height: f64) {
    let (Some(label), Some(main)) = (with(&app, |s| s.label), main_window(&app)) else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let size = (
        (width * scale).ceil() as i64,
        (height * scale).ceil() as i64,
    );
    let at = place::tip_position(label, size, whole_screen(&main));
    let _ = window.set_size(LogicalSize::new(width.ceil(), height.ceil()));
    let _ = window.set_position(PhysicalPosition::new(at.0 as i32, at.1 as i32));
    let _ = window.show();
    track_hover(app, label);
}

#[tauri::command]
fn balance_providers() -> Vec<String> {
    core::providers()
}

/// Save: the balance the provider's billing page shows right now.
#[tauri::command(async)]
fn balance_save(app: AppHandle, provider: String, amount: String) -> Result<(), String> {
    let value = format::parse_amount(&amount).ok_or("Enter a number, e.g. 25.00")?;
    let today = core::utc_day(Clock::system().now);
    let spent = with(&app, |s| {
        view::today_spend(s.last_data.as_ref(), &provider, &today)
    });
    core::set_balance(&provider, Some(value), spent)?;
    close_balance(&app);
    refresh(&app);
    Ok(())
}

/// Clear: stop estimating this provider's balance.
#[tauri::command(async)]
fn balance_clear(app: AppHandle, provider: String) -> Result<(), String> {
    core::set_balance(&provider, None, None)?;
    close_balance(&app);
    refresh(&app);
    Ok(())
}

#[tauri::command]
fn balance_cancel(app: AppHandle) {
    close_balance(&app);
}

// --- start ----------------------------------------------------------------------

fn build_windows(app: &tauri::App, topmost: bool) -> tauri::Result<WebviewWindow> {
    let page = |name: &str| WebviewUrl::App(name.into());
    // The webview keeps its cache with the tracker's other files, not in a folder of its own.
    let webview_data = core::store_dir().join("webview");
    // The card: no frame, no taskbar button, slightly see-through (the page draws it at 96%).
    let main = WebviewWindowBuilder::new(app, "main", page("index.html"))
        .data_directory(webview_data.clone())
        .title("AI Usage")
        .inner_size(290.0, 420.0)
        .decorations(false)
        .shadow(false)
        .resizable(USER_RESIZABLE)
        .skip_taskbar(true)
        .always_on_top(topmost)
        .visible(false);
    #[cfg(not(target_os = "macos"))]
    let main = main.transparent(true);
    let main = main.build()?;

    // The hover card for "● N working": made once, shown and hidden as the pointer comes and goes.
    WebviewWindowBuilder::new(app, "tip", page("tip.html"))
        .data_directory(webview_data.clone())
        .title("AI Usage: working sessions")
        .inner_size(420.0, 160.0)
        .decorations(false)
        .shadow(false)
        .resizable(USER_RESIZABLE)
        .skip_taskbar(true)
        .always_on_top(true)
        .focused(false)
        .visible(false)
        .build()?;

    // "Set API balance…": also made once; closing it only hides it.
    let balance = WebviewWindowBuilder::new(app, "balance", page("balance.html"))
        .data_directory(webview_data.clone())
        .title("Set API balance")
        .inner_size(340.0, 196.0)
        .resizable(USER_RESIZABLE)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .visible(false)
        .build()?;
    let dialog = balance.clone();
    balance.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = dialog.hide();
        }
    });
    Ok(main)
}

fn start(app: &tauri::App) -> tauri::Result<()> {
    let handle = app.handle().clone();
    let settings = Settings::load(&core::store_dir().join("widget.json"));
    let (topmost, alerts) = (
        settings.flag("topmost", true),
        settings.flag("alerts", true),
    );

    // If the exe moved since "Start with Windows" was turned on, the entry follows it.
    let mut entries = autostart::system();
    if let Ok(exe) = std::env::current_exe() {
        let _ = autostart::refresh(&mut entries, &exe);
    }
    app.manage(context_menu(
        &handle,
        topmost,
        alerts,
        autostart::is_enabled(&entries),
    )?);
    app.manage(Mutex::new(Shared {
        settings,
        life: Lifecycle::default(),
        last_data: None,
        status: Status {
            text: "Scanning…".into(),
            color: MUTED,
        },
        working: String::new(),
        label: None,
        dragging: false,
        moves: None,
    }));
    let saver = save_drags(handle.clone());
    with(&handle, |s| s.moves = Some(saver));

    let main = build_windows(app, topmost)?;
    move_to(&handle, &main, start_position(&handle, &main));
    let events = handle.clone();
    main.on_window_event(move |event| match event {
        WindowEvent::Moved(at) => {
            let saver = with(&events, |s| s.moves.clone().filter(|_| s.dragging));
            if let Some(saver) = saver {
                let _ = saver.send((at.x, at.y));
            }
        }
        WindowEvent::CloseRequested { .. } => events.exit(0),
        _ => {}
    });
    main.show()?;

    // Without a tray icon the widget must stay visible, or it could never be reached again.
    match tray::build(&handle) {
        Ok(()) => with(&handle, |s| s.life.tray_available = true),
        Err(_) => on_tray(
            &handle,
            TrayEvent::Error("Tray icon unavailable; tracker kept visible".into()),
        ),
    }

    refresh(&handle);
    tick(&handle);
    for (every, job) in [
        (REFRESH, refresh as fn(&AppHandle)),
        (COUNTDOWN, tick as fn(&AppHandle)),
    ] {
        let app = handle.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(every);
            job(&app);
        });
    }
    Ok(())
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            screen,
            drag,
            press,
            menu,
            fit,
            working_enter,
            tip_ready,
            balance_providers,
            balance_save,
            balance_clear,
            balance_cancel
        ])
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .setup(|app| Ok(start(app)?))
        .run(tauri::generate_context!());
    if let Err(error) = app {
        // No console on Windows; this is for a terminal on other systems.
        eprintln!("AI Usage Tracker could not start: {error}");
        std::process::exit(1);
    }
}

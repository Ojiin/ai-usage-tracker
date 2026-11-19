//! The Rust port of `tracker.py`.
//!
//! Replace each `unimplemented!` with the port of the source function. Return `Err` where
//! the source throws or raises: the caller gets an error there too. After changing a
//! signature, run `rust-anything refresh` to regenerate the bindings. `rust-anything diff` checks
//! behavior against what the source recorded.

#![forbid(unsafe_code)]

/// Port of `local_day` (`tracker.py:42`).
pub fn local_day(ts: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &ts;
    unimplemented!("port `local_day` from tracker.py")
}

/// Port of `local_time` (`tracker.py:53`).
pub fn local_time(ts: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &ts;
    unimplemented!("port `local_time` from tracker.py")
}

/// Port of `claude_tool_events` (`tracker.py:62`).
pub fn claude_tool_events(
    name: serde_json::Value,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let _ = (&name, &args);
    unimplemented!("port `claude_tool_events` from tracker.py")
}

/// Port of `scan_commands` (`tracker.py:78`).
pub fn scan_commands(events: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &events;
    unimplemented!("port `scan_commands` from tracker.py")
}

/// Port of `working_now` (`tracker.py:96`).
///
/// `window` defaults to `120` in the source.
///
/// `now` defaults to `None` in the source.
pub fn working_now(
    window: i64,
    now: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let _ = (&window, &now);
    unimplemented!("port `working_now` from tracker.py")
}

/// Port of `tail_rows` (`tracker.py:104`).
///
/// `size` defaults to `512 * 1024` in the source.
pub fn tail_rows(
    path: serde_json::Value,
    size: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let _ = (&path, &size);
    unimplemented!("port `tail_rows` from tracker.py")
}

/// Port of `pretty_model` (`tracker.py:125`).
pub fn pretty_model(model: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &model;
    unimplemented!("port `pretty_model` from tracker.py")
}

/// Port of `short_text` (`tracker.py:138`).
///
/// `limit` defaults to `90` in the source.
pub fn short_text(text: serde_json::Value, limit: i64) -> Result<serde_json::Value, String> {
    let _ = (&text, &limit);
    unimplemented!("port `short_text` from tracker.py")
}

/// Port of `codex_titles` (`tracker.py:143`).
pub fn codex_titles() -> Result<serde_json::Value, String> {
    unimplemented!("port `codex_titles` from tracker.py")
}

/// Port of `codex_db_working` (`tracker.py:168`).
pub fn codex_db_working(now: f64) -> Result<serde_json::Value, String> {
    let _ = &now;
    unimplemented!("port `codex_db_working` from tracker.py")
}

/// Port of `claude_session_info` (`tracker.py:216`).
pub fn claude_session_info(path: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &path;
    unimplemented!("port `claude_session_info` from tracker.py")
}

/// Port of `working_details` (`tracker.py:264`).
///
/// `window` defaults to `120` in the source.
///
/// `now` defaults to `None` in the source.
pub fn working_details(
    window: i64,
    now: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let _ = (&window, &now);
    unimplemented!("port `working_details` from tracker.py")
}

/// Port of `describe_session` (`tracker.py:327`).
pub fn describe_session(info: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &info;
    unimplemented!("port `describe_session` from tracker.py")
}

/// Port of `scan_claude` (`tracker.py:366`).
///
/// `events` defaults to `None` in the source.
pub fn scan_claude(events: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let _ = &events;
    unimplemented!("port `scan_claude` from tracker.py")
}

/// Port of `scan_codex` (`tracker.py:418`).
///
/// `events` defaults to `None` in the source.
pub fn scan_codex(events: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let _ = &events;
    unimplemented!("port `scan_codex` from tracker.py")
}

/// Port of `to_epoch` (`tracker.py:525`).
pub fn to_epoch(iso: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &iso;
    unimplemented!("port `to_epoch` from tracker.py")
}

/// Port of `pretty_plan` (`tracker.py:532`).
pub fn pretty_plan(raw: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &raw;
    unimplemented!("port `pretty_plan` from tracker.py")
}

/// Port of `window_label` (`tracker.py:536`).
pub fn window_label(minutes: f64) -> Result<serde_json::Value, String> {
    let _ = &minutes;
    unimplemented!("port `window_label` from tracker.py")
}

/// Port of `codex_live` (`tracker.py:557`).
pub fn codex_live() -> Result<serde_json::Value, String> {
    unimplemented!("port `codex_live` from tracker.py")
}

/// Port of `codex_details` (`tracker.py:593`).
pub fn codex_details(body: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &body;
    unimplemented!("port `codex_details` from tracker.py")
}

/// Port of `claude_live` (`tracker.py:613`).
pub fn claude_live() -> Result<serde_json::Value, String> {
    unimplemented!("port `claude_live` from tracker.py")
}

/// Port of `codex_log_limits` (`tracker.py:650`).
pub fn codex_log_limits(snapshot: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &snapshot;
    unimplemented!("port `codex_log_limits` from tracker.py")
}

/// Port of `load_pricing` (`tracker.py:694`).
pub fn load_pricing() -> Result<serde_json::Value, String> {
    unimplemented!("port `load_pricing` from tracker.py")
}

/// Port of `cost_of` (`tracker.py:702`).
pub fn cost_of(
    rec: serde_json::Value,
    pricing: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let _ = (&rec, &pricing);
    unimplemented!("port `cost_of` from tracker.py")
}

/// Port of `aggregate` (`tracker.py:709`).
///
/// `events` defaults to `()` in the source.
pub fn aggregate(
    records: serde_json::Value,
    pricing: serde_json::Value,
    events: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let _ = (&records, &pricing, &events);
    unimplemented!("port `aggregate` from tracker.py")
}

/// link probe
pub mod native {
    pub fn probe() -> std::path::PathBuf {
        limits_watch_rs::port::native::store()
    }
}

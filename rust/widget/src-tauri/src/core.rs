//! The one seam between the widget and the ported core (tracker, api_billing, limits_watch).
//!
//! This is the ONLY file that may call the core crates under `rust/`. Everything else in the
//! widget goes through the functions here, so wiring a newly ported crate is a change to one
//! function body in this file.
//!
//! What is wired today: `limits-watch-rs` (`store_dir`, `due_alerts`, `pace_text`).
//! What is not: everything from `tracker-rs` and `api-billing-rs`. Each such function is marked
//! `TODO(core)` with the call it is waiting for. Until then it answers from the JSON file named
//! by `AIUT_FAKE_DATA`, or returns an error the widget shows in its status line.
//!
//! Fake-data mode is permanent: it is how the page is developed and screenshotted.
//!
//! ```text
//! AIUT_FAKE_DATA=fixtures/sample-full.json npm run dev
//! ```
//!
//! The file holds what `tracker.collect()` returns (`scripts/make_fixture.py` writes one from
//! made-up logs), plus one optional extra key, `_working_details`: the list `working_details()`
//! would return, each session carrying its `describe_session()` text as `_describe`.
//! In this mode nothing is read from the real logs and no alert state is written to the real
//! store folder.

use limits_watch_rs::port::native as limits;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The environment variable that turns fake-data mode on.
pub const FAKE_DATA: &str = "AIUT_FAKE_DATA";

fn fake_path() -> Option<PathBuf> {
    std::env::var_os(FAKE_DATA)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// The fake file, when fake-data mode is on. An unreadable file is an error, like a failed scan.
fn fake() -> Option<Result<Value, String>> {
    let path = fake_path()?;
    let read = std::fs::read_to_string(&path)
        .map_err(|error| format!("OSError: {}: {error}", path.display()))
        .and_then(|text| {
            serde_json::from_str::<Value>(&text).map_err(|error| format!("ValueError: {error}"))
        });
    Some(read)
}

fn not_wired(what: &str) -> String {
    format!("{what} core not wired; set {FAKE_DATA}")
}

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64())
}

// --- places -------------------------------------------------------------------

/// `%LOCALAPPDATA%\AIUsageTracker`: where widget.json and the core's own state files live.
pub fn store_dir() -> PathBuf {
    limits::store()
}

/// The folder the exe is in (`tracker.HERE`): dashboard.html is written there, and the core
/// reads pricing.json from there.
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

/// `widget.DASHBOARD`.
pub fn dashboard_path() -> PathBuf {
    exe_dir().join("dashboard.html")
}

// --- tracker ------------------------------------------------------------------

/// `tracker.collect(log)`: one full scan, as the dict the Python function returns. The widget
/// reads these keys: `today`, `now_epoch`, `day[]{key,total,cost,calls}`, `tool[]{key,total}`,
/// `daily_tool`, `daily_tool_cost`, `daily_tool_unpriced`, `has_pricing`, `plan_limits`, `api`,
/// `openai_analytics{summary.lifetimeTokens,daily}`, `warnings`.
///
/// The real one must read pricing.json from [`exe_dir`] and already includes
/// `limits_watch.record_and_project`, as `tracker.collect` does.
pub fn collect(log: &mut dyn FnMut(&str)) -> Result<Value, String> {
    // TODO(core): wire to tracker_rs::port::native::collect(log) -> Result<Value, String>
    let _ = &log;
    match fake() {
        Some(data) => data.map(|mut data| {
            if let Some(map) = data.as_object_mut() {
                map.shift_remove("_working_details");
            }
            rebase(&mut data, unix_now());
            data
        }),
        None => Err(not_wired("tracker")),
    }
}

/// `tracker.write_dashboard(data, out_path)`: template.html with the scan data put in.
pub fn write_dashboard(data: &Value, path: &Path) -> Result<(), String> {
    // TODO(core): wire to tracker_rs::port::native::write_dashboard(data: &Value, out_path: &Path) -> Result<(), String>
    if fake_path().is_none() {
        return Err(not_wired("dashboard"));
    }
    let template = include_str!("../../../../template.html");
    let json = serde_json::to_string(data).map_err(|error| format!("ValueError: {error}"))?;
    let page = template.replace("/*__DATA__*/null", &json.replace('<', "\\u003c"));
    std::fs::write(path, page).map_err(|error| format!("OSError: {error}"))
}

/// `tracker.working_now()`: `{"Claude Code": n, "Codex": n}`, the sessions working right now.
pub fn working_now() -> Result<Value, String> {
    // TODO(core): wire to tracker_rs::port::native::working_now() -> Result<Value, String>
    let mut active = json!({"Claude Code": 0, "Codex": 0});
    for info in working_details()? {
        if let Some(tool) = info.get("tool").and_then(Value::as_str) {
            active[tool] = json!(active[tool].as_i64().unwrap_or(0) + 1);
        }
    }
    Ok(active)
}

/// `tracker.working_details()`: one dict per working session, newest first.
pub fn working_details() -> Result<Vec<Value>, String> {
    // TODO(core): wire to tracker_rs::port::native::working_details() -> Result<Vec<Value>, String>
    match fake() {
        Some(data) => Ok(data?
            .get("_working_details")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()),
        None => Err(not_wired("tracker")),
    }
}

/// `tracker.describe_session(info)`: "Opus 5.5 med working on scripts" plus detail lines,
/// separated by newlines.
pub fn describe_session(info: &Value) -> Result<String, String> {
    // TODO(core): wire to tracker_rs::port::native::describe_session(info: &Value) -> Result<String, String>
    match info.get("_describe").and_then(Value::as_str) {
        Some(text) => Ok(text.to_string()),
        None => Err(not_wired("tracker")),
    }
}

// --- limits_watch (wired) -----------------------------------------------------

/// `limits_watch.pace_text(projection, fmt_time)`.
pub fn pace_text(projection: Option<&Value>, fmt_time: &dyn Fn(i64) -> String) -> String {
    limits::pace_text(projection, fmt_time)
}

/// `limits_watch.due_alerts(entries, now)`: `(title, text)` for each threshold crossed since the
/// last call. It also remembers what was crossed, so call it on every scan, alerts on or off.
pub fn due_alerts(
    plan_limits: &Value,
    now: f64,
    fmt_time: &dyn Fn(i64) -> String,
) -> Vec<(String, String)> {
    let state = match fake_path() {
        // Sample data must never mark real thresholds as already alerted.
        Some(_) => std::env::temp_dir()
            .join("AIUsageTracker-fake")
            .join("limit_alerts.json"),
        None => store_dir().join("limit_alerts.json"),
    };
    limits::due_alerts(plan_limits, now, &state, fmt_time)
}

// --- api_billing --------------------------------------------------------------

/// `list(api_billing.PROVIDERS)`: the choices in "Set API balance…".
pub fn providers() -> Vec<String> {
    // TODO(core): wire to api_billing_rs::port::native::providers() -> Vec<String>
    vec!["Anthropic".into(), "OpenAI".into()]
}

/// `api_billing.utc_day(datetime.now().astimezone())`: today's date in UTC, the key of the
/// providers' daily reports.
pub fn utc_day(now: f64) -> String {
    // TODO(core): wire to api_billing_rs::port::native::utc_day(now: f64) -> String
    chrono::DateTime::from_timestamp(now.floor() as i64, 0)
        .map(|at| at.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// `api_billing.set_balance(provider, amount, today_spend)`; `amount: None` clears the balance.
pub fn set_balance(
    provider: &str,
    amount: Option<f64>,
    today_spend: Option<f64>,
) -> Result<(), String> {
    // TODO(core): wire to api_billing_rs::port::native::set_balance(provider: &str, amount: Option<f64>, today_spend: Option<f64>) -> Result<(), String>
    let _ = (provider, amount, today_spend);
    match fake_path() {
        Some(_) => Ok(()), // the sample file is never changed
        None => Err(not_wired("billing")),
    }
}

// --- fake-data mode only --------------------------------------------------------

/// Moves the sample scan to "now": every epoch and ISO time shifts by the same amount, so the
/// countdowns run and the pace is fresh. Day-keyed tables (and so "today") stay as recorded.
fn rebase(data: &mut Value, now: f64) {
    let Some(recorded) = data.get("now_epoch").and_then(Value::as_f64) else {
        return;
    };
    let shift = now.floor() as i64 - recorded as i64;
    let epoch = |v: &mut Value| {
        if let Some(t) = v.as_i64() {
            *v = json!(t + shift);
        }
    };
    let iso = |v: &mut Value| {
        let parsed = v
            .as_str()
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok());
        if let Some(at) = parsed {
            let moved = at.to_utc() + chrono::TimeDelta::seconds(shift);
            *v = json!(moved.format("%Y-%m-%dT%H:%M:%SZ").to_string());
        }
    };
    let each = |list: Option<&mut Value>, f: &mut dyn FnMut(&mut Value)| {
        list.and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
            .for_each(f);
    };
    if let Some(map) = data.as_object_mut() {
        map.get_mut("now_epoch").into_iter().for_each(epoch);
        each(map.get_mut("plan_limits"), &mut |entry| {
            entry.get_mut("as_of").into_iter().for_each(iso);
            each(entry.get_mut("windows"), &mut |w| {
                w.get_mut("resets_at").into_iter().for_each(epoch);
                w.get_mut("as_of").into_iter().for_each(iso);
                let hits = w.get_mut("projection").and_then(|p| p.get_mut("hits_at"));
                hits.into_iter().for_each(epoch);
            });
        });
        each(map.get_mut("api"), &mut |provider| {
            provider.get_mut("as_of").into_iter().for_each(iso);
            let balance = provider.get_mut("balance").and_then(|b| b.get_mut("as_of"));
            balance.into_iter().for_each(iso);
        });
        let account = map
            .get_mut("openai_analytics")
            .and_then(|a| a.get_mut("as_of"));
        account.into_iter().for_each(iso);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebase_shifts_every_time_by_the_same_amount() {
        let mut data = json!({
            "now_epoch": 1_800_000_000, "today": "2027-01-15",
            "plan_limits": [{"as_of": "2027-01-15T08:00:00.000Z", "windows": [
                {"resets_at": 1_800_003_600, "projection": {"hits_at": 1_800_001_800, "pct": 82.0}},
                {"resets_at": null}]}],
            "api": [{"as_of": "2027-01-15T07:00:00Z", "balance": {"as_of": "2027-01-10T08:00:00Z"}},
                    {"balance": null}],
            "openai_analytics": {"as_of": null},
        });
        rebase(&mut data, 1_800_000_100.9);
        assert_eq!(data["now_epoch"], 1_800_000_100);
        assert_eq!(data["today"], "2027-01-15");
        let limits = &data["plan_limits"][0];
        assert_eq!(limits["as_of"], "2027-01-15T08:01:40Z");
        assert_eq!(limits["windows"][0]["resets_at"], 1_800_003_700);
        assert_eq!(
            limits["windows"][0]["projection"]["hits_at"],
            1_800_001_900
        );
        assert_eq!(limits["windows"][0]["projection"]["pct"], 82.0);
        assert_eq!(limits["windows"][1]["resets_at"], Value::Null);
        assert_eq!(data["api"][0]["as_of"], "2027-01-15T07:01:40Z");
        assert_eq!(data["api"][0]["balance"]["as_of"], "2027-01-10T08:01:40Z");
    }

    #[test]
    fn utc_day_and_providers() {
        assert_eq!(utc_day(1_800_000_000.0), "2027-01-15");
        assert_eq!(utc_day(1_800_000_000.0 + 16.0 * 3600.0), "2027-01-16");
        assert_eq!(providers(), ["Anthropic", "OpenAI"]);
    }

    #[test]
    fn pace_text_comes_from_the_ported_core() {
        let clock = |t: i64| format!("T{t}");
        assert_eq!(
            pace_text(Some(&json!({"hits_at": 5})), &clock),
            "At this pace: 100% by T5"
        );
        assert_eq!(
            pace_text(Some(&json!({"at_reset": 60.8})), &clock),
            "On pace for ~61% by reset"
        );
        assert_eq!(pace_text(None, &clock), "");
    }
}

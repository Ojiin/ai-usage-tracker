//! The Rust port of `limits_watch.py`: plan-limit history, pace projection.
//!
//! Values stay `serde_json::Value` where the original is dynamically typed, so wrong shapes fail
//! with the error Python raises. `record_and_project_at` is the entry for Rust callers: it takes
//! the history path and updates the entries in place, as the original does.

#![forbid(unsafe_code)]

use crate::pynum::Num;
pub use native::{due_alerts, pace_text, record_and_project_at, store};
use pycompat::{dumps, get, iso_micros, iter, py_str, round_to, to_float, truthy, ty};
use serde_json::{Map, Value, json};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

fn num_ty(n: Num) -> &'static str {
    if matches!(n, Num::Int(_)) {
        "int"
    } else {
        "float"
    }
}

const KEEP_SECONDS: f64 = 8.0 * 86400.0; // weekly windows need a week of samples
const SAME_WINDOW: f64 = 15.0 * 60.0; // resets_at jitter between polls of the same window
const MIN_SPAN: f64 = 10.0 * 60.0; // least time between samples to trust a rate
const MAX_AGE: f64 = 30.0 * 60.0; // no projection from a reading older than this
const THRESHOLDS: [i64; 3] = [80, 95, 100];

/// Port of `to_epoch` (`limits_watch.py:23`): whole seconds, or `None` for anything unreadable.
pub fn to_epoch(iso: serde_json::Value) -> Result<Option<i64>, String> {
    Ok(iso
        .as_str()
        .and_then(iso_micros)
        .map(|micros| micros / 1_000_000))
}

/// Port of `same_window` (`limits_watch.py:45`).
pub fn same_window(a: Option<f64>, b: Option<f64>) -> Result<bool, String> {
    Ok(matches!((a, b), (Some(a), Some(b)) if (a - b).abs() <= SAME_WINDOW))
}

/// Port of `lookback` (`limits_watch.py:49`): an hour of history for short windows (or a long
/// one about to reset), a day otherwise.
pub fn lookback(now: f64, resets_at: f64) -> Result<i64, String> {
    Ok(if resets_at - now <= 5.0 * 3600.0 {
        3600
    } else {
        86400
    })
}

fn number(v: &Value, op: &str, other: Num) -> Result<f64, String> {
    let message = || {
        format!(
            "TypeError: '{op}' not supported between instances of '{}' and '{}'",
            ty(v),
            num_ty(other)
        )
    };
    v.as_f64().ok_or_else(message)
}

/// One `[t, pct, resets_at]` sample, with Python's error for any other shape.
fn sample(s: &Value, now: Num, resets_at: Num) -> Result<(f64, f64, Option<f64>), String> {
    let minus = |left: &Value| {
        let (left, right) = (ty(left), num_ty(resets_at));
        format!("TypeError: unsupported operand type(s) for -: '{left}' and '{right}'")
    };
    match s {
        Value::Array(a) if a.len() >= 3 => {
            let r = match &a[2] {
                Value::Null => None,
                reset => Some(reset.as_f64().ok_or_else(|| minus(reset))?),
            };
            Ok((number(&a[0], ">=", now)?, number(&a[1], "<", now)?, r))
        }
        Value::Array(_) => Err("IndexError: list index out of range".into()),
        Value::String(c) if c.chars().count() < 3 => {
            Err("IndexError: string index out of range".into())
        }
        Value::String(_) => Err(minus(s)),
        other => Err(format!(
            "TypeError: '{}' object is not subscriptable",
            ty(other)
        )),
    }
}

/// Port of `project` (`limits_watch.py:54`): `{"pct", "rate_per_hour", "hits_at"}` when 100%
/// comes before the reset, else `{"pct", "rate_per_hour", "at_reset"}`; `None` without a
/// reliable pace.
pub fn project(
    samples: serde_json::Value,
    now: Num,
    resets_at: Num,
) -> Result<serde_json::Value, String> {
    let (now_f, reset) = (now.f(), resets_at.f());
    if reset == 0.0 || reset <= now_f {
        return Ok(Value::Null);
    }
    let floor = now_f - lookback(now_f, reset)? as f64;
    let mut window = Vec::new();
    for s in iter(&samples)? {
        let (t, pct, r) = sample(&s, now, resets_at)?;
        if same_window(r, Some(reset))? && t >= floor {
            window.push((t, pct, r, s[1].clone()));
        }
    }
    window.sort_by(|a, b| {
        (a.0, a.1, a.2)
            .partial_cmp(&(b.0, b.1, b.2))
            .map_or(Ordering::Equal, |o| o)
    });
    // A drop inside one window means the service corrected itself; only trust what came after.
    if let Some(i) = (1..window.len())
        .rev()
        .find(|&i| window[i].1 < window[i - 1].1)
    {
        window.drain(..i);
    }
    let (Some(first), Some(last), true) = (window.first(), window.last(), window.len() >= 2) else {
        return Ok(Value::Null);
    };
    let ((t0, p0), (t1, p1)) = ((first.0, first.1), (last.0, last.1));
    let rate = (p1 - p0) / (t1 - t0);
    if t1 - t0 < MIN_SPAN || now_f - t1 > MAX_AGE || p1 >= 100.0 || rate.is_nan() || rate <= 0.0 {
        return Ok(Value::Null);
    }
    let mut out = Map::new();
    out.insert("pct".into(), last.3.clone());
    out.insert("rate_per_hour".into(), json!(round_to(rate * 3600.0, 2)));
    let hits_at = t1 + (100.0 - p1) / rate;
    if hits_at < reset {
        out.insert("hits_at".into(), json!(hits_at as i64));
    } else {
        let at_reset = p1 + rate * (reset - t1);
        out.insert(
            "at_reset".into(),
            if at_reset > 100.0 {
                json!(100)
            } else {
                json!(round_to(at_reset, 1))
            },
        );
    }
    Ok(Value::Object(out))
}

/// Port of `window_key` (`limits_watch.py:86`): `service|label`.
pub fn window_key(entry: serde_json::Value, w: serde_json::Value) -> Result<String, String> {
    let service = match &entry {
        Value::Object(m) if !m.contains_key("$num") => {
            m.get("service").ok_or("KeyError: service")?
        }
        Value::Array(_) => {
            return Err("TypeError: list indices must be integers or slices, not str".into());
        }
        Value::String(_) => {
            return Err("TypeError: string indices must be integers, not 'str'".into());
        }
        other => {
            return Err(format!(
                "TypeError: '{}' object is not subscriptable",
                ty(other)
            ));
        }
    };
    get(&w, "label")?;
    let label = w.get("label").map_or_else(|| "?".to_string(), py_str);
    Ok(format!("{}|{label}", py_str(service)))
}

fn windows(entry: &Value) -> Result<Vec<Value>, String> {
    get(entry, "windows")?
        .filter(|w| truthy(w))
        .map_or_else(|| Ok(Vec::new()), iter)
}

/// For Rust callers: the bridge only sees the functions at the top of this file.
pub mod native {
    use super::*;

    /// `%LOCALAPPDATA%\AIUsageTracker`, or the home folder where that is not set.
    pub fn store() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        base.or_else(std::env::home_dir)
            .unwrap_or_default()
            .join("AIUsageTracker")
    }

    /// The JSON object saved at `path`; empty when the file is missing or unreadable.
    fn load(path: &Path) -> Map<String, Value> {
        let text = std::fs::read_to_string(path).ok();
        match text.and_then(|text| serde_json::from_str(&text).ok()) {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        }
    }

    /// Like the original, a file that cannot be written is not an error.
    fn save(path: &Path, data: &Map<String, Value>) {
        let folder = path.parent().map_or(Ok(()), std::fs::create_dir_all);
        let _ = folder.and_then(|()| std::fs::write(path, dumps(&Value::Object(data.clone()))));
    }

    /// Saves this scan's readings in the history at `path` and sets `w["projection"]` on each
    /// window of a live entry that has a reliable pace.
    pub fn record_and_project_at(entries: &mut Value, now: Num, path: &Path) -> Result<(), String> {
        let mut history = load(path);
        for entry in iter(entries)? {
            for w in windows(&entry)? {
                let as_of = get(&w, "as_of")?
                    .filter(|v| truthy(v))
                    .or(get(&entry, "as_of")?);
                let t = to_epoch(as_of.cloned().map_or(Value::Null, |v| v))?;
                let (Some(t), Some(used)) = (t, get(&w, "used_percent")?) else {
                    continue;
                };
                let key = window_key(entry.clone(), w.clone())?;
                let Value::Array(samples) = history.entry(key).or_insert_with(|| json!([])) else {
                    continue;
                };
                if !samples.iter().any(|s| s[0].as_i64() == Some(t)) {
                    samples.push(json!([t, to_float(used)?, w.get("resets_at")]));
                }
            }
        }
        let oldest = now.f() - KEEP_SECONDS;
        history.retain(|_, samples| {
            let mut kept: Vec<Value> = iter(samples).unwrap_or_default();
            kept.retain(|s| s[0].as_f64().is_some_and(|t| t >= oldest));
            let key = |s: &Value| (s[0].as_f64(), s[1].as_f64(), s[2].as_f64());
            kept.sort_by(|a, b| key(a).partial_cmp(&key(b)).map_or(Ordering::Equal, |o| o));
            *samples = Value::Array(kept);
            samples.as_array().is_some_and(|kept| !kept.is_empty())
        });
        save(path, &history);
        for entry in entries.as_array_mut().into_iter().flatten() {
            let (live, service) = (entry.get("source") == Some(&json!("live")), entry.clone());
            for w in entry
                .get_mut("windows")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                if let Value::Object(map) = w {
                    map.shift_remove("projection");
                }
                let reset = w
                    .get("resets_at")
                    .and_then(|r| r.as_i64().map(Num::Int).or(r.as_f64().map(Num::Float)));
                if let (true, Some(reset)) = (live, reset) {
                    let samples = history
                        .get(&window_key(service.clone(), w.clone())?)
                        .cloned();
                    let projection = project(samples.unwrap_or_else(|| json!([])), now, reset)?;
                    if let (false, Value::Object(map)) = (projection.is_null(), w) {
                        map.insert("projection".into(), projection);
                    }
                }
            }
        }
        Ok(())
    }
    /// Port of `pace_text` (`limits_watch.py:116`).
    pub fn pace_text(projection: Option<&Value>, fmt_time: &dyn Fn(i64) -> String) -> String {
        let Some(p) = projection.filter(|p| truthy(p)) else {
            return String::new();
        };
        match p.get("hits_at") {
            Some(at) => format!(
                "At this pace: 100% by {}",
                fmt_time(at.as_i64().unwrap_or_default())
            ),
            None => format!(
                "On pace for ~{:.0}% by reset",
                p["at_reset"].as_f64().unwrap_or_default()
            ),
        }
    }

    /// Port of `due_alerts` (`limits_watch.py:124`): `(title, text)` for each threshold crossed
    /// since the last call, once per window. Only fresh live readings count.
    pub fn due_alerts(
        entries: &Value,
        now: f64,
        path: &Path,
        fmt_time: &dyn Fn(i64) -> String,
    ) -> Vec<(String, String)> {
        let (mut state, mut alerts) = (load(path), Vec::new());
        for entry in entries.as_array().into_iter().flatten() {
            if entry.get("source") != Some(&json!("live")) || entry.get("error").is_some_and(truthy)
            {
                continue;
            }
            for w in entry
                .get("windows")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let (Some(reset), Ok(key)) = (
                    w.get("resets_at").filter(|r| truthy(r)),
                    window_key(entry.clone(), w.clone()),
                ) else {
                    continue;
                };
                let (reset_f, pct) = (
                    reset.as_f64().unwrap_or_default(),
                    w.get("used_percent")
                        .and_then(Value::as_f64)
                        .unwrap_or_default(),
                );
                if reset_f <= now {
                    continue;
                }
                let seen = state.get(&key);
                let same = same_window(seen.and_then(|s| s["resets_at"].as_f64()), Some(reset_f))
                    == Ok(true);
                let mut level = seen
                    .filter(|_| same)
                    .and_then(|s| s["level"].as_i64())
                    .unwrap_or_default();
                if let Some(top) = THRESHOLDS
                    .iter()
                    .copied()
                    .rfind(|&t| level < t && t as f64 <= pct)
                {
                    let (service, at) = (py_str(&entry["service"]), fmt_time(reset_f as i64));
                    let label = w.get("label").map_or_else(|| "limit".to_string(), py_str);
                    let pace = pace_text(w.get("projection"), fmt_time);
                    alerts.push(if top >= 100 {
                        (
                            format!("{service} limit reached"),
                            format!("{label} limit reached. Resets {at}."),
                        )
                    } else if pace.is_empty() {
                        (
                            format!("{service} {top}% used"),
                            format!("{label} at {pct:.0}%. Resets {at}."),
                        )
                    } else {
                        (
                            format!("{service} {top}% used"),
                            format!("{label} at {pct:.0}%. {pace}; resets {at}."),
                        )
                    });
                    level = top;
                }
                state.insert(key, json!({"resets_at": reset, "level": level}));
            }
        }
        save(path, &state);
        alerts
    }
}

/// Port of `record_and_project` (`limits_watch.py:90`), on the default history file. Through
/// the bridge `path` is only ever a value that is not a `Path`, and fails as it does in Python.
pub fn record_and_project(
    entries: serde_json::Value,
    now: Num,
    path: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    if let Some(wrong) = path.filter(truthy) {
        return Err(format!(
            "AttributeError: '{}' object has no attribute 'read_text'",
            ty(&wrong)
        ));
    }
    let mut entries = entries;
    record_and_project_at(&mut entries, now, &store().join("limits_history.json"))?;
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;
    const RESET: i64 = NOW + 3 * 3600;

    #[test]
    fn reads_iso_times() {
        assert_eq!(to_epoch(json!("2027-01-15T08:00:00+00:00")), Ok(Some(NOW)));
        assert_eq!(to_epoch(json!("2027-01-15T08:00:00.75Z")), Ok(Some(NOW)));
        assert_eq!(to_epoch(json!("2027-01-15 10:00+02:00")), Ok(Some(NOW)));
        assert_eq!(to_epoch(json!("1969-12-31T23:59:59.5+00:00")), Ok(Some(0)));
        for bad in [
            json!("x"),
            json!(null),
            json!(20270115),
            json!("2027-01-15T"),
            json!("2027-02-30"),
        ] {
            assert_eq!(to_epoch(bad), Ok(None));
        }
    }

    #[test]
    fn saves_readings_and_projects_live_windows() {
        let dir = std::env::temp_dir().join(format!("limits-watch-rs-{}", std::process::id()));
        let path = dir.join("nested").join("history.json");
        let entry = |pct: i64, as_of: &str, source: &str| {
            json!([{"service": "Claude Code", "source": source, "as_of": as_of,
                    "windows": [{"label": "5-hour session", "used_percent": pct, "resets_at": RESET, "projection": 1}]}])
        };
        let mut first = entry(40, "2027-01-15T07:30:00+00:00", "live");
        assert_eq!(
            record_and_project_at(&mut first, Num::Int(NOW - 1800), &path),
            Ok(())
        );
        assert_eq!(first[0]["windows"][0].get("projection"), None);
        let mut second = entry(60, "2027-01-15T08:00:00+00:00", "live");
        assert_eq!(
            record_and_project_at(&mut second, Num::Int(NOW), &path),
            Ok(())
        );
        assert_eq!(
            second[0]["windows"][0]["projection"],
            json!({"pct": 60.0, "rate_per_hour": 40.0, "hits_at": NOW + 3600})
        );
        let mut cached = entry(60, "2027-01-15T08:00:00+00:00", "cached");
        assert_eq!(
            record_and_project_at(&mut cached, Num::Int(NOW + 60), &path),
            Ok(())
        );
        assert_eq!(cached[0]["windows"][0].get("projection"), None);
        let saved = std::fs::read_to_string(&path).map_err(|e| e.to_string());
        let expected = format!(
            r#"{{"Claude Code|5-hour session": [[{}, 40.0, {RESET}], [{NOW}, 60.0, {RESET}]]}}"#,
            NOW - 1800
        );
        assert_eq!(saved, Ok(expected));
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn alerts_fire_once_per_threshold_and_window() {
        let dir =
            std::env::temp_dir().join(format!("limits-watch-rs-alerts-{}", std::process::id()));
        let path = dir.join("alerts.json");
        let entry = |pct: i64, reset: i64, source: &str, extra: Value| {
            let mut w = json!({"label": "5-hour session", "used_percent": pct, "resets_at": reset});
            w.as_object_mut()
                .into_iter()
                .for_each(|m| m.extend(extra.as_object().cloned().unwrap_or_default()));
            json!([{"service": "Claude Code", "source": source, "windows": [w]}])
        };
        let titles = |entries: Value, now: i64| -> Vec<String> {
            let fired = due_alerts(&entries, now as f64, &path, &|_| "T".into());
            fired
                .into_iter()
                .map(|(title, text)| format!("{title}: {text}"))
                .collect()
        };
        let none: Vec<String> = Vec::new();
        assert_eq!(titles(entry(50, RESET, "live", json!({})), NOW), none);
        let pace = json!({"projection": {"pct": 82, "rate_per_hour": 20, "hits_at": NOW + 3240}});
        let with_pace =
            "Claude Code 80% used: 5-hour session at 82%. At this pace: 100% by T; resets T.";
        assert_eq!(titles(entry(82, RESET, "live", pace), NOW), [with_pace]);
        assert_eq!(titles(entry(85, RESET, "live", json!({})), NOW), none);
        let reached = "Claude Code limit reached: 5-hour session limit reached. Resets T.";
        assert_eq!(titles(entry(100, RESET, "live", json!({})), NOW), [reached]);
        let next = "Claude Code 80% used: 5-hour session at 81%. Resets T.";
        assert_eq!(
            titles(entry(81, RESET + 5 * 3600, "live", json!({})), RESET + 60),
            [next]
        );
        assert_eq!(
            titles(entry(99, RESET + 40 * 3600, "cached", json!({})), NOW),
            none
        );
        assert_eq!(titles(entry(99, NOW - 10, "live", json!({})), NOW), none);
        let _ = std::fs::remove_dir_all(dir);
    }
}

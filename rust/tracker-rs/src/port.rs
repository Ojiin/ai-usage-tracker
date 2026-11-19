//! The Rust port of `tracker.py`: scans Claude Code and Codex logs, reads plan limits, and
//! builds the dashboard data.
//!
//! Values stay `serde_json::Value` where the original is dynamically typed, so a wrong shape
//! fails with the error Python raises. The functions at the top of the file are the bridged
//! exports; Rust callers use [`native`], whose [`Dirs`] holds the folders the original keeps
//! in module globals.

#![forbid(unsafe_code)]

use crate::pynum::Num;
use chrono::{Datelike, Local, TimeZone, Timelike};
use indexmap::IndexMap;
pub use native::Dirs;
use pycompat::{capitalize, dumps, dumps_ascii, float_floor_div, float_repr, get, hash_key};
use pycompat::{int_of, is_space, iso_micros, item, iter, name_of, no_attr, posix_name};
use pycompat::{read_json, rglob, round_to, split_ws, splitlines, stem, str_of, truthy, ty};
use pycompat::{windows_name, with_suffix};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use limits_watch_rs::port as limits_watch;

const FIELDS: [&str; 4] = ["input", "cache_write", "cache_read", "output"];
const TAIL: i64 = 512 * 1024;
const CODEX_STALE_TURN: i64 = 30 * 60; // an inProgress turn this long without activity crashed
const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const ZERO: Num = Num::Int(0);
static NULL: Value = Value::Null;

/// Python's `v.get(key)`: `None` for a missing key.
fn at<'a>(v: &'a Value, key: &str) -> Result<&'a Value, String> {
    Ok(get(v, key)?.unwrap_or(&NULL))
}

/// Python's `(v or {}).get(key)`.
fn sub<'a>(v: &'a Value, key: &str) -> Result<&'a Value, String> {
    if truthy(v) { at(v, key) } else { Ok(&NULL) }
}

/// Python's `a or b`.
fn or<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    if truthy(a) { a } else { b }
}

/// Python's `str(v or default)`.
fn text_or(v: &Value, default: &str) -> String {
    if truthy(v) { str_of(v) } else { default.into() }
}

fn tuple(items: Vec<Value>) -> Value {
    json!({ "$tuple": items })
}

fn num(v: &Value) -> Result<Num, String> {
    let n = match v {
        Value::Bool(b) => Some(Num::Int(i64::from(*b))),
        Value::Number(n) => n.as_i64().map(Num::Int).or(n.as_f64().map(Num::Float)),
        _ => None,
    };
    n.ok_or_else(|| format!("TypeError: unsupported operand type(s): '{}'", ty(v)))
}

/// Python's `v or 0` as a number.
fn num_or_zero(v: &Value) -> Result<Num, String> {
    if truthy(v) { num(v) } else { Ok(ZERO) }
}

fn value(n: Num) -> Value {
    match n {
        Num::Int(i) => json!(i),
        Num::Float(x) => json!(x),
    }
}

/// Python's `m.get(key, 0)` as a number.
fn count(m: &Value, key: &str) -> Result<Num, String> {
    match m {
        Value::Object(map) => map.get(key).map_or(Ok(ZERO), num),
        other => Err(no_attr(other, "get")),
    }
}

/// Python's `sum(items)`: exact while the items are ints, then Neumaier's compensated sum.
fn py_sum(items: &[Num]) -> Num {
    let (mut whole, mut at) = (0_i64, 0);
    while let Some(Num::Int(i)) = items.get(at) {
        let Some(sum) = whole.checked_add(*i) else {
            break;
        };
        (whole, at) = (sum, at + 1);
    }
    let Some(first) = items.get(at) else {
        return Num::Int(whole);
    };
    let (mut sum, mut lost) = (whole as f64 + first.f(), 0.0);
    for item in &items[at + 1..] {
        let (x, t) = (item.f(), sum + item.f());
        if matches!(item, Num::Float(_)) {
            lost += if sum.abs() >= x.abs() {
                (sum - t) + x
            } else {
                (x - t) + sum
            };
        }
        sum = t;
    }
    let exact = lost != 0.0 && lost.is_finite();
    Num::Float(if exact { sum + lost } else { sum })
}

fn clock() -> f64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH);
    now.map_or(0.0, |d| d.as_secs_f64())
}

/// `datetime.fromisoformat(ts).timestamp()`; `None` for anything else.
fn epoch(ts: &Value) -> Option<f64> {
    let text = ts.as_str()?;
    let micros = iso_micros(text)?;
    if text.get(10..).is_some_and(|t| t.contains(['+', '-', 'Z'])) {
        return Some(micros as f64 / 1e6);
    }
    Some(micros.div_euclid(1_000_000) as f64 + micros.rem_euclid(1_000_000) as f64 / 1e6)
}

/// `to_epoch(ts) or 0`
fn epoch_or_zero(ts: &Value) -> f64 {
    epoch(ts).unwrap_or(0.0)
}

/// `local_time`: `[local YYYY-MM-DD, hour, weekday]` of an ISO timestamp, or three nulls.
fn local_time_of(ts: &Value) -> [Value; 3] {
    let at = ts.as_str().and_then(iso_micros);
    match at.and_then(|micros| Local.timestamp_micros(micros).single()) {
        Some(at) => {
            let day = at.format("%Y-%m-%d").to_string();
            [
                json!(day),
                json!(at.hour()),
                json!(at.weekday().num_days_from_monday()),
            ]
        }
        None => [Value::Null, Value::Null, Value::Null],
    }
}

/// `local_day`: the local day of an ISO timestamp; null for none or an unreadable one.
fn local_day_of(ts: &Value) -> Result<Value, String> {
    match ts {
        _ if !truthy(ts) => Ok(Value::Null),
        Value::String(_) => Ok(local_time_of(ts)[0].clone()),
        other => Err(no_attr(other, "replace")),
    }
}

/// Port of `local_day` (`tracker.py:42`): ISO timestamp to the local `YYYY-MM-DD`, or `None`.
pub fn local_day(ts: serde_json::Value) -> Result<serde_json::Value, String> {
    local_day_of(&ts)
}

/// Port of `local_time` (`tracker.py:53`): `(local day, hour, weekday)` or three `None`s.
pub fn local_time(ts: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(tuple(local_time_of(&ts).to_vec()))
}

fn event(tool: &str, day: &Value, kind: &str, name: Value) -> Value {
    json!({"tool": tool, "day": day, "kind": kind, "name": name})
}

fn tool_events(name: &str, args: &Value) -> Result<Vec<(&'static str, Value)>, String> {
    if let Some(rest) = name.strip_prefix("mcp__") {
        let named = name.matches("__").count() >= 2;
        let server = if named {
            name.split("__").nth(1).unwrap_or(rest)
        } else {
            rest
        };
        return Ok(vec![
            ("tool", json!(format!("MCP: {server}"))),
            ("mcp", json!(server)),
        ]);
    }
    let mut events = vec![("tool", json!(name))];
    if name == "Agent" || name == "Task" {
        let kind = or(at(args, "subagent_type")?, &json!("general-purpose")).clone();
        events.push(("agent", kind));
    } else if name == "Skill" && truthy(at(args, "skill")?) {
        events.push(("skill", json!(str_of(&args["skill"]))));
    }
    Ok(events)
}

/// Port of `claude_tool_events` (`tracker.py:62`): the tool, plus agent type, skill or MCP
/// server.
pub fn claude_tool_events(
    name: serde_json::Value,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let text = name.as_str().ok_or_else(|| no_attr(&name, "startswith"))?;
    let pairs = tool_events(text, &args)?.into_iter();
    Ok(pairs
        .map(|(kind, name)| tuple(vec![json!(kind), name]))
        .collect())
}

/// `^/[A-Za-z][\w:.-]*$`
fn is_command(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next() == Some('/')
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_alphanumeric() || "_:.-".contains(c))
}

/// `datetime.fromtimestamp(t).strftime("%Y-%m-%d")`; `None` where Python raises.
fn day_of(t: f64) -> Option<String> {
    let at = Local.timestamp_opt(t.floor() as i64, 0).single();
    let at = at.filter(|at| t.is_finite() && (1..=9999).contains(&at.year()))?;
    Some(at.format("%Y-%m-%d").to_string())
}

/// Port of `scan_commands` (`tracker.py:78`): the events are appended to the list in Python;
/// here only the failure of appending to anything else shows.
pub fn scan_commands(events: serde_json::Value) -> Result<serde_json::Value, String> {
    let mut found = Vec::new();
    dirs().scan_commands(&mut found, &mut Vec::new())?;
    if found.is_empty() || events.is_array() {
        return Ok(Value::Null);
    }
    Err(no_attr(&events, "append"))
}

/// The sessions for the `now` of a bridged function, which may be anything.
fn working(window: i64, now: Option<Value>) -> Result<Vec<Value>, String> {
    let now = now.unwrap_or(Value::Null);
    let now = match now.get("$num").and_then(Value::as_str) {
        Some("NaN") => Some(f64::NAN),
        Some("Infinity") => Some(f64::INFINITY),
        Some("-Infinity") => Some(f64::NEG_INFINITY),
        Some(_) => None,
        None if !truthy(&now) => None,
        None => match num(&now) {
            Ok(now) => Some(now.f()),
            // Python fails at the first subtraction from `now`, where a NaN fails here.
            Err(_) => {
                let odd = format!(
                    "unsupported operand type(s) for -: '{}' and 'float'",
                    ty(&now)
                );
                let found = dirs().working_details(window, Some(f64::NAN));
                return found.map_err(|_| format!("TypeError: {odd}"));
            }
        },
    };
    dirs().working_details(window, now)
}

/// Port of `working_now` (`tracker.py:96`): working sessions per tool.
pub fn working_now(
    window: i64,
    now: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    Ok(native::count_tools(&working(window, now)?))
}

/// The file descriptors Python has read and closed.
static CLOSED: std::sync::Mutex<Vec<Value>> = std::sync::Mutex::new(Vec::new());

/// A path argument of a bridged function. Python takes a number as a file descriptor, reads it
/// and closes it: only standard input can be read, and only once.
fn path_arg(path: &Value) -> Result<Option<PathBuf>, String> {
    match path {
        Value::String(text) => Ok(Some(PathBuf::from(text))),
        Value::Bool(_) | Value::Number(_) => {
            let mut closed = CLOSED.lock().map_err(|e| format!("RuntimeError: {e}"))?;
            let first = !closed.contains(path);
            closed.push(path.clone());
            Ok((first && *path == json!(0)).then(|| PathBuf::from("/dev/fd/0")))
        }
        other => {
            let ty = ty(other);
            Err(format!(
                "TypeError: expected str, bytes or os.PathLike object, not {ty}"
            ))
        }
    }
}

/// Port of `tail_rows` (`tracker.py:104`): the JSON rows in the last `size` bytes of a log.
pub fn tail_rows(
    path: serde_json::Value,
    size: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let Some(path) = path_arg(&path)?.filter(|p| std::fs::File::open(p).is_ok()) else {
        return Ok(json!([]));
    };
    let size = match size {
        None => TAIL,
        Some(Value::Bool(b)) => i64::from(b),
        Some(Value::Number(n)) if !n.is_f64() => n.as_i64().unwrap_or(i64::MAX),
        Some(other) => {
            let odd = format!(
                "unsupported operand type(s) for -: 'int' and '{}'",
                ty(&other)
            );
            return Err(format!("TypeError: {odd}"));
        }
    };
    Ok(Value::Array(native::tail_rows(&path, size)))
}

fn pretty_model_of(model: &Value) -> Result<String, String> {
    let name = match model {
        _ if !truthy(model) => return Ok("unknown model".into()),
        Value::String(text) => text,
        other => return Err(no_attr(other, "startswith")),
    };
    let Some(rest) = name.strip_prefix("claude-") else {
        return Ok(name.clone());
    };
    let digits = |part: &&str| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
    let (version, words): (Vec<&str>, Vec<&str>) = rest.split('-').partition(digits);
    let mut out: Vec<String> = words.into_iter().map(capitalize).collect();
    if !version.is_empty() {
        out.push(version.join("."));
    }
    Ok(out.join(" "))
}

/// Port of `pretty_model` (`tracker.py:125`): `claude-opus-5-5` to `Opus 5.5`.
pub fn pretty_model(model: serde_json::Value) -> Result<serde_json::Value, String> {
    pretty_model_of(&model).map(Value::String)
}

fn short_text_of(text: &Value, limit: i64) -> Value {
    let source = text_or(text, "");
    let text = split_ws(&source).join(" ");
    let len = text.chars().count() as i64;
    if len <= limit {
        return json!(text);
    }
    let keep = if limit < 1 {
        (len + limit - 1).max(0)
    } else {
        limit - 1
    };
    json!(
        text.chars()
            .take(keep as usize)
            .chain(['…'])
            .collect::<String>()
    )
}

/// Port of `short_text` (`tracker.py:138`): one line of at most `limit` characters.
pub fn short_text(text: serde_json::Value, limit: i64) -> Result<serde_json::Value, String> {
    Ok(short_text_of(&text, limit))
}

/// Port of `codex_titles` (`tracker.py:143`): thread names by session id.
pub fn codex_titles() -> Result<serde_json::Value, String> {
    dirs().codex_titles()
}

/// Port of `codex_db_working` (`tracker.py:168`): `(sessions, ids the databases track)`.
pub fn codex_db_working(now: f64) -> Result<serde_json::Value, String> {
    let (sessions, covered) = dirs().codex_db_working(now)?;
    Ok(tuple(vec![json!(sessions), json!({ "$set": covered })]))
}

/// The text Python can append to a `str`.
fn text(v: &Value) -> Result<&str, String> {
    let odd = || {
        format!(
            "TypeError: can only concatenate str (not \"{}\") to str",
            ty(v)
        )
    };
    v.as_str().ok_or_else(odd)
}

/// The text of a path Python builds a `Path` from.
fn path_text(cwd: &Value) -> Result<&str, String> {
    let odd = || format!("TypeError: argument should be a str, not '{}'", ty(cwd));
    cwd.as_str().ok_or_else(odd)
}

/// `info["project"] = PureWindowsPath(cwd).name if cwd`
fn set_project(project: &mut Value, cwd: &Value) -> Result<(), String> {
    if truthy(cwd) {
        *project = json!(windows_name(path_text(cwd)?));
    }
    Ok(())
}

/// Port of `claude_session_info` (`tracker.py:216`).
pub fn claude_session_info(path: serde_json::Value) -> Result<serde_json::Value, String> {
    native::claude_session_info(&path_arg(&path)?.unwrap_or_default())
}

/// `" ".join(c.get("text", "") for c in content or [] if isinstance(c, dict))`
fn joined_text(content: &Value) -> Result<String, String> {
    let mut parts = Vec::new();
    let items = if truthy(content) {
        iter(content)?
    } else {
        Vec::new()
    };
    for c in items.iter().filter_map(Value::as_object) {
        let text = c.get("text").map_or(Some(""), Value::as_str);
        let odd = || {
            format!(
                "TypeError: sequence item {}: expected str instance",
                parts.len()
            )
        };
        parts.push(text.ok_or_else(odd)?.to_string());
    }
    Ok(parts.join(" "))
}

/// Port of `working_details` (`tracker.py:264`): what each working session is doing.
pub fn working_details(
    window: i64,
    now: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    working(window, now).map(Value::Array)
}

/// `" ".join(x for x in parts if x)`
fn join_words(parts: &[&Value]) -> Result<String, String> {
    let mut words = Vec::new();
    for part in parts.iter().filter(|p| truthy(p)) {
        let odd = || {
            format!(
                "TypeError: sequence item {}: expected str instance",
                words.len()
            )
        };
        words.push(part.as_str().ok_or_else(odd)?);
    }
    Ok(words.join(" "))
}

/// `EFFORT.get(effort, effort)`
fn effort_of(effort: &Value) -> Result<Value, String> {
    hash_key(effort)?;
    Ok(match effort.as_str() {
        Some("medium") => json!("med"),
        Some("minimal") => json!("min"),
        _ => effort.clone(),
    })
}

/// `pretty_model(model) if model else other`
fn model_or(model: &Value, other: &Value) -> Result<Value, String> {
    if truthy(model) {
        pretty_model_of(model).map(Value::String)
    } else {
        Ok(other.clone())
    }
}

/// Port of `describe_session` (`tracker.py:327`): the widget's hover card for one session.
pub fn describe_session(info: serde_json::Value) -> Result<serde_json::Value, String> {
    native::describe_session(&info).map(Value::String)
}

/// What a bridged scan returns once Python has appended `found` events to its `events`.
fn appended(events: Option<Value>, found: &[Value], out: Value) -> Result<Value, String> {
    match events {
        Some(odd) if !odd.is_null() && !odd.is_array() && !found.is_empty() => {
            Err(no_attr(&odd, "extend"))
        }
        _ => Ok(out),
    }
}

/// Port of `scan_claude` (`tracker.py:366`): one record per API response.
pub fn scan_claude(events: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let mut found = Vec::new();
    let wanted = events.as_ref().is_some_and(|e| !e.is_null());
    let records = dirs().scan_claude(wanted.then_some(&mut found), &mut Vec::new())?;
    appended(events, &found, json!(records))
}

/// Port of `scan_codex` (`tracker.py:418`): `(records, newest rate limits per limit id)`.
pub fn scan_codex(events: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let mut found = Vec::new();
    let wanted = events.as_ref().is_some_and(|e| !e.is_null());
    let (records, limits) = dirs().scan_codex(wanted.then_some(&mut found), &mut Vec::new())?;
    appended(events, &found, tuple(vec![json!(records), limits]))
}

/// Port of `to_epoch` (`tracker.py:525`): seconds since 1970 as a float, or `None`.
pub fn to_epoch(iso: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(json!(epoch(&iso)))
}

/// Port of `pretty_plan` (`tracker.py:532`): `pro_plus` to `Pro Plus`.
pub fn pretty_plan(raw: serde_json::Value) -> Result<serde_json::Value, String> {
    let text = match &raw {
        _ if !truthy(&raw) => return Ok(Value::Null),
        Value::String(text) => text.replace('_', " "),
        other => return Err(no_attr(other, "replace")),
    };
    let words: Vec<String> = split_ws(&text).into_iter().map(capitalize).collect();
    Ok(json!(words.join(" ")))
}

/// Port of `window_label` (`tracker.py:536`): `10080` to `7-day`.
pub fn window_label(minutes: crate::pynum::Num) -> Result<serde_json::Value, String> {
    let part = |unit: i64, name: &str| match minutes {
        Num::Int(m) if m.rem_euclid(unit) == 0 => Some(format!("{}-{name}", m.div_euclid(unit))),
        Num::Float(m) if m % unit as f64 == 0.0 => Some(format!(
            "{}-{name}",
            float_repr(float_floor_div(m, unit as f64))
        )),
        _ => None,
    };
    let minute = || match minutes {
        Num::Int(m) => format!("{m}-minute"),
        Num::Float(m) => format!("{}-minute", float_repr(m)),
    };
    if minutes.f() == 0.0 {
        return Ok(json!("Usage window"));
    }
    Ok(json!(
        part(1440, "day")
            .or_else(|| part(60, "hour"))
            .unwrap_or_else(minute)
    ))
}

/// One limit window of the dashboard: `name` is the bucket it belongs to.
fn limit_window(name: &Value, minutes: Num, w: &Value, reset: &str) -> Result<Value, String> {
    let mut label = window_label(minutes)?;
    if name != "codex" {
        label = json!(format!("{}: {}", str_of(name), str_of(&label)));
    }
    Ok(json!({"label": label, "used_percent": w["used_percent"], "resets_at": w[reset]}))
}

fn pair((live, error): (Value, Value)) -> Value {
    tuple(vec![live, error])
}

/// Port of `codex_live` (`tracker.py:557`): `(reading, None)` or `(None, why not)`.
pub fn codex_live() -> Result<serde_json::Value, String> {
    dirs().codex_live_with(&native::get_json).map(pair)
}

/// Port of `codex_details` (`tracker.py:593`): credit and limit states as text.
pub fn codex_details(body: serde_json::Value) -> Result<serde_json::Value, String> {
    let mut details = Vec::new();
    let credits = at(&body, "credits")?;
    if truthy(sub(credits, "unlimited")?) {
        details.push("Credits: unlimited".to_string());
    } else if !credits["balance"].is_null() {
        details.push(format!("Credit balance: {}", str_of(&credits["balance"])));
    } else if credits["has_credits"] == false {
        details.push("No additional credits available".into());
    }
    let rl = or(&body["rate_limit"], &body);
    if truthy(at(rl, "limit_reached")?) || truthy(&body["rate_limit_reached_type"]) {
        details.push("Usage limit reached".into());
    }
    if rl["allowed"] == false {
        details.push("Usage currently blocked by the service".into());
    }
    if truthy(&body["spend_control_reached"]) {
        details.push("Workspace spend control reached".into());
    }
    Ok(json!(details))
}

/// Port of `claude_live` (`tracker.py:613`): `(reading, None)` or `(None, why not)`.
pub fn claude_live() -> Result<serde_json::Value, String> {
    dirs()
        .claude_live_with(clock(), &native::get_json)
        .map(pair)
}

/// Port of `codex_log_limits` (`tracker.py:650`): the newest rate limits in the Codex logs.
pub fn codex_log_limits(snapshot: serde_json::Value) -> Result<serde_json::Value, String> {
    let odd = || {
        format!(
            "TypeError: argument of type '{}' is not iterable",
            ty(&snapshot)
        )
    };
    let buckets: Vec<&Value> = match &snapshot {
        _ if !truthy(&snapshot) => return Ok(Value::Null),
        Value::Object(map) if map.contains_key("timestamp") => vec![&snapshot],
        Value::Object(map) => map.values().collect(),
        Value::String(_) | Value::Array(_) => return Err(no_attr(&snapshot, "values")),
        _ => return Err(odd()),
    };
    let (mut newest, mut newest_at, mut windows) = (&NULL, f64::NEG_INFINITY, Vec::new());
    for bucket in &buckets {
        let stamp = epoch_or_zero(at(bucket, "timestamp")?);
        if stamp > newest_at {
            (newest, newest_at) = (bucket, stamp);
        }
    }
    for bucket in buckets {
        let name = or(
            &bucket["limit_name"],
            or(&bucket["limit_id"], &json!("codex")),
        )
        .clone();
        for w in [&bucket["primary"], &bucket["secondary"]] {
            if truthy(w) && !at(w, "used_percent")?.is_null() {
                let minutes = num_or_zero(&w["window_minutes"])?;
                let mut window = limit_window(&name, minutes, w, "resets_at")?;
                window["as_of"] = bucket["timestamp"].clone();
                windows.push(window);
            }
        }
    }
    let plan = pretty_plan(at(newest, "plan_type")?.clone())?;
    let details = codex_details(newest.clone())?;
    Ok(json!({"plan": plan, "windows": windows, "details": details,
              "as_of": newest["timestamp"], "source": "log"}))
}

/// Port of `load_pricing` (`tracker.py:694`), read from the repository the crate sits in.
pub fn load_pricing() -> Result<serde_json::Value, String> {
    dirs().load_pricing()
}

fn cost(rec: &Value, pricing: &Value) -> Result<Option<f64>, String> {
    at(pricing, "")?;
    let model = item(rec, "model")?;
    hash_key(model)?;
    let rate = model.as_str().and_then(|m| pricing.get(m));
    let Some(rate) = rate.filter(|r| truthy(r)) else {
        return Ok(None);
    };
    let mut parts = Vec::new();
    for f in FIELDS {
        let tokens = num(item(rec, f)?)?;
        parts.push(tokens * get(rate, f)?.map_or(Ok(ZERO), num)?);
    }
    Ok(Some(py_sum(&parts).f() / 1e6))
}

/// Port of `cost_of` (`tracker.py:702`): API-equivalent dollars of one record, `None` unpriced.
pub fn cost_of(
    rec: serde_json::Value,
    pricing: serde_json::Value,
) -> Result<serde_json::Value, String> {
    Ok(json!(cost(&rec, &pricing)?))
}

#[derive(Default)]
struct Bucket {
    fields: [Option<Num>; 4],
    reasoning: Option<Num>,
    total: Option<Num>,
    cost: f64,
    unpriced: bool,
    calls: i64,
    sessions: HashSet<String>,
}

/// Sums by name, in the order the names first appear.
type Sums = IndexMap<String, Option<Num>>;

fn add(slot: &mut Option<Num>, n: Num) {
    *slot = Some(slot.map_or(n, |sum| sum + n));
}

fn key_text(v: &Value) -> Result<String, String> {
    let odd = || format!("TypeError: a '{}' key is not supported", ty(v));
    v.as_str().map(str::to_string).ok_or_else(odd)
}

fn total(n: Option<Num>) -> Value {
    value(n.unwrap_or(ZERO))
}

/// `{k: v}` in the given order.
fn object<T>(pairs: impl IntoIterator<Item = (String, T)>, to: impl Fn(T) -> Value) -> Value {
    Value::Object(pairs.into_iter().map(|(k, v)| (k, to(v))).collect())
}

/// The five largest sums, largest first.
fn top(sums: Sums) -> Value {
    let mut rows: Vec<(String, Num)> = Vec::new();
    rows.extend(sums.into_iter().map(|(k, n)| (k, n.unwrap_or(ZERO))));
    rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    rows.into_iter()
        .take(5)
        .map(|(k, n)| json!([k, value(n)]))
        .collect()
}

/// The folders of the bridged functions: the pricing file sits in the repository, where
/// `tracker.py` finds it.
fn dirs() -> Dirs {
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    Dirs {
        here,
        ..Dirs::from_env()
    }
}

/// For Rust callers: the bridge only sees the functions at the top of this file.
pub mod native {
    use super::*;

    /// What a log line is given to: `Ok(false)` stops the reading.
    type Each<'a> = &'a mut dyn FnMut(Value) -> Result<bool, String>;
    /// `log` of the original: one line of progress.
    pub type Log<'a> = &'a mut dyn FnMut(&str);
    /// `get_json`, replaceable in tests: `(url, headers, app) -> (body, error)`.
    pub type Fetch<'a> = &'a dyn Fn(&str, &[(&str, String)], &str) -> (Value, Value);
    /// `api_billing.api_usage` or `openai_analytics.collect`: given the log, gives its data.
    pub type Part<'a> = &'a mut dyn FnMut(Log) -> Value;
    /// A live check of `plan_limits`: `(reading, error)` for a service.
    pub type Check<'a> = &'a mut dyn FnMut(&str) -> Result<(Value, Value), String>;
    /// `(reading, error)`, or the error Python raises.
    pub type Reading = Result<(Value, Value), String>;

    /// The dashboard page, as the original bundles `template.html`.
    pub const TEMPLATE: &str = include_str!("../template.html");

    /// The folders the original computes when it is imported.
    pub struct Dirs {
        /// `CLAUDE_CONFIG_DIR`, or `~/.claude`.
        pub claude: PathBuf,
        /// `CODEX_HOME`, or `~/.codex`.
        pub codex: PathBuf,
        /// Where `pricing.json` sits: beside the program.
        pub here: PathBuf,
        /// `%LOCALAPPDATA%\AIUsageTracker`: the limits cache and history.
        pub store: PathBuf,
    }

    /// `*.jsonl`, as pathlib matches it: any case on Windows.
    fn is_log(name: &str) -> bool {
        name.ends_with(".jsonl") || cfg!(windows) && name.to_lowercase().ends_with(".jsonl")
    }

    /// `iter_jsonl`: every JSON object line of a log, `.zst` logs decompressed as they are read.
    /// A file that cannot be read to its end adds a warning.
    pub fn iter_jsonl(path: &Path, warnings: &mut Vec<String>, each: Each) -> Result<(), String> {
        let zst = path.extension().is_some_and(|e| e == "zst");
        // The exception that ended the reading, or the one `each` raised.
        type Stop = Result<&'static str, String>;
        let failed = |decoding: bool, e: std::io::Error| -> Stop {
            Ok(match e.kind() {
                _ if decoding => "ZstdError",
                std::io::ErrorKind::NotFound => "FileNotFoundError",
                std::io::ErrorKind::PermissionDenied => "PermissionError",
                std::io::ErrorKind::IsADirectory => "IsADirectoryError",
                _ => "OSError",
            })
        };
        let mut read = || -> Result<(), Stop> {
            let file = std::fs::File::open(path).map_err(|e| failed(false, e))?;
            let mut lines: Box<dyn BufRead> = if zst {
                let stream = zstd::Decoder::new(file).map_err(|e| failed(true, e))?;
                Box::new(BufReader::new(stream))
            } else {
                Box::new(BufReader::new(file))
            };
            let mut line = Vec::new();
            loop {
                line.clear();
                match lines.read_until(b'\n', &mut line) {
                    Ok(0) => return Ok(()),
                    // The Python reader ends quietly where a compressed log is cut off.
                    Err(e) if zst && e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
                    Err(e) => return Err(failed(zst, e)),
                    Ok(_) => {}
                }
                for piece in String::from_utf8_lossy(&line).split('\r') {
                    // A running session may end with an incomplete line.
                    if let Ok(row @ Value::Object(_)) = serde_json::from_str(piece)
                        && !each(row).map_err(Err)?
                    {
                        return Ok(());
                    }
                }
            }
        };
        match read() {
            Err(Err(error)) => Err(error),
            Err(Ok(why)) => {
                let name = name_of(path);
                warnings.push(format!(
                    "Could not fully read {name} ({why}); totals may be incomplete."
                ));
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    /// `tail_rows`: the JSON rows in the last `size` bytes of a log, oldest first; a cut-off
    /// first line is skipped.
    pub fn tail_rows(path: &Path, size: i64) -> Vec<Value> {
        let read = || -> std::io::Result<(Vec<u8>, bool)> {
            let mut file = std::fs::File::open(path)?;
            let len = file.seek(SeekFrom::End(0))?;
            let start = (i128::from(len) - i128::from(size)).max(0) as u64;
            file.seek(SeekFrom::Start(start))?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok((bytes, start > 0))
        };
        let Ok((bytes, cut)) = read() else {
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&bytes);
        let lines = splitlines(&text).into_iter().skip(usize::from(cut));
        let rows = lines.filter_map(|line| serde_json::from_str::<Value>(line).ok());
        rows.filter(Value::is_object).collect()
    }

    /// `claude_session_info`: model, effort, project, title and last prompt of a Claude Code log.
    pub fn claude_session_info(path: &Path) -> Result<Value, String> {
        let mut info = json!({"tool": "Claude Code", "model": null, "effort": null,
                              "project": null, "title": null, "task": null});
        for row in tail_rows(path, TAIL) {
            set_project(&mut info["project"], &row["cwd"])?;
            let (kind, model) = (&row["type"], sub(&row["message"], "model").unwrap_or(&NULL));
            if kind == "assistant" && !model.is_null() && model != "<synthetic>" {
                info["model"] = model.clone();
                info["effort"] = or(&row["effort"], &info["effort"]).clone();
            } else if kind == "custom-title" && truthy(&row["customTitle"]) {
                info["title"] = row["customTitle"].clone();
            } else if kind == "agent-name" && truthy(&row["agentName"]) && !truthy(&info["title"]) {
                info["title"] = row["agentName"].clone();
            } else if kind == "last-prompt" && truthy(&row["lastPrompt"]) {
                info["task"] = short_text_of(&row["lastPrompt"], 90);
            }
        }
        Ok(info)
    }

    /// `codex_session_info`: the same for a Codex rollout log, with its thread ids.
    pub fn codex_session_info(path: &Path, titles: &Value) -> Result<Value, String> {
        let mut info = json!({"tool": "Codex", "model": null, "effort": null, "project": null,
                              "title": null, "task": null, "id": null, "parent": null,
                              "nickname": null, "source": null});
        let mut quiet = Vec::new();
        // session_meta is the first line
        iter_jsonl(path, &mut quiet, &mut |row| {
            let meta = &row["payload"];
            let meta = if row["type"] == "session_meta" && meta.is_object() {
                meta
            } else {
                &NULL
            };
            info["id"] = meta["id"].clone();
            info["parent"] = meta["parent_thread_id"].clone();
            info["nickname"] = meta["agent_nickname"].clone();
            info["source"] = meta["thread_source"].clone();
            set_project(&mut info["project"], &meta["cwd"]).map(|()| false)
        })?;
        let read = |info: &mut Value, row: &Value| -> Result<(), String> {
            let payload = if row["payload"].is_object() {
                &row["payload"]
            } else {
                &NULL
            };
            let said = payload["type"] == "message" && payload["role"] == "user";
            if row["type"] == "turn_context" {
                info["model"] = or(&payload["model"], &info["model"]).clone();
                info["effort"] = or(&payload["effort"], &info["effort"]).clone();
                set_project(&mut info["project"], &payload["cwd"])?;
            } else if row["type"] == "response_item" && said {
                let text = joined_text(&payload["content"])?;
                let shown = text.trim_start_matches(is_space);
                // skip injected context blocks
                if !shown.is_empty() && !shown.starts_with('<') {
                    info["task"] = short_text_of(&json!(text), 90);
                }
            }
            Ok(())
        };
        for row in tail_rows(path, TAIL) {
            read(&mut info, &row)?;
        }
        if !truthy(&info["model"]) {
            // turn contexts are rare in long subagent logs; look through the whole file
            iter_jsonl(path, &mut quiet, &mut |row| {
                read(&mut info, &row).map(|()| true)
            })?;
        }
        info["title"] = info["id"]
            .as_str()
            .and_then(|id| titles.get(id))
            .cloned()
            .into();
        Ok(info)
    }

    /// `describe_session`: `Opus 5.5 med working on scripts` plus detail lines.
    pub fn describe_session(info: &Value) -> Result<String, String> {
        let effort = effort_of(at(info, "effort")?)?;
        let head = join_words(&[&model_or(&info["model"], &info["tool"])?, &effort])?;
        let place = text_or(&info["project"], "an unknown project");
        let mut lines = vec![format!("{head} working on {place}")];
        if truthy(&info["title"]) {
            lines.push(format!("  “{}”", str_of(&info["title"])));
        }
        if truthy(&info["task"]) {
            lines.push(format!("  Last prompt: {}", str_of(&info["task"])));
        }
        for sub in info.get("subagents").map_or(Ok(Vec::new()), iter)? {
            let model = model_or(at(&sub, "model")?, &NULL)?;
            let model = join_words(&[&model, &effort_of(&sub["effort"])?])?;
            let mut line = format!("  ↳ {}", str_of(item(&sub, "type")?));
            if !model.is_empty() {
                line += &format!(" · {model}");
            }
            if truthy(&sub["description"]) {
                line += &format!(": {}", str_of(&sub["description"]));
            }
            lines.push(line);
        }
        let (tool, age) = (str_of(item(info, "tool")?), str_of(item(info, "age")?));
        lines.push(format!("  {tool} · last write {age}s ago"));
        Ok(lines.join("\n"))
    }

    /// `aggregate`: totals per day, model, project and tool, the hourly and yearly breakdowns,
    /// and the activity counts. `events` is `None` for the default, no events.
    pub fn aggregate(
        records: &Value,
        pricing: &Value,
        events: Option<&Value>,
    ) -> Result<Value, String> {
        let mut by: [IndexMap<String, Bucket>; 4] = Default::default();
        let mut daily_tool: BTreeMap<String, Sums> = BTreeMap::new();
        let mut daily_cost: BTreeMap<String, IndexMap<String, f64>> = BTreeMap::new();
        let mut daily_unpriced: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut day_hours: BTreeMap<String, IndexMap<String, Vec<Option<Num>>>> = BTreeMap::new();
        let mut year_top: IndexMap<String, [Sums; 2]> = IndexMap::new();
        for r in iter(records)? {
            if !truthy(item(&r, "day")?) {
                continue;
            }
            let c = cost(&r, pricing)?;
            let mut fields = [ZERO; 4];
            for (slot, f) in fields.iter_mut().zip(FIELDS) {
                *slot = num(item(&r, f)?)?;
            }
            let sum = py_sum(&fields);
            let (day, tool) = (key_text(&r["day"])?, key_text(item(&r, "tool")?)?);
            add(
                daily_tool
                    .entry(day.clone())
                    .or_default()
                    .entry(tool.clone())
                    .or_default(),
                sum,
            );
            if !at(&r, "hour")?.is_null() {
                let hours = day_hours
                    .entry(day.clone())
                    .or_default()
                    .entry(tool.clone());
                let hour = r["hour"]
                    .as_i64()
                    .ok_or("TypeError: list indices must be integers")?;
                let at = usize::try_from(if hour < 0 { hour + 24 } else { hour }).ok();
                let slot = at.and_then(|at| hours.or_insert_with(|| vec![None; 24]).get_mut(at));
                add(slot.ok_or("IndexError: list index out of range")?, sum);
            }
            let model = format!("{}|{tool}", str_of(&r["model"]));
            let project = format!("{}|{tool}", str_of(item(&r, "project")?));
            let year = year_top.entry(day.chars().take(4).collect()).or_default();
            add(year[0].entry(model.clone()).or_default(), sum);
            add(year[1].entry(project.clone()).or_default(), sum);
            match c {
                None => drop(
                    daily_unpriced
                        .entry(day.clone())
                        .or_default()
                        .insert(tool.clone()),
                ),
                Some(c) => {
                    *daily_cost
                        .entry(day.clone())
                        .or_default()
                        .entry(tool.clone())
                        .or_default() += c
                }
            }
            let session = format!("{tool}|{}", hash_key(item(&r, "session")?)?);
            for (dim, key) in by.iter_mut().zip([&day, &model, &project, &tool]) {
                let b = dim.entry(key.clone()).or_default();
                for (slot, n) in b.fields.iter_mut().zip(fields) {
                    add(slot, n);
                }
                add(&mut b.total, sum);
                // already included in output, never add twice
                add(&mut b.reasoning, r.get("reasoning").map_or(Ok(ZERO), num)?);
                b.calls += 1;
                b.sessions.insert(session.clone());
                b.unpriced |= c.is_none();
                b.cost += c.unwrap_or(0.0);
            }
        }
        let mut out = Map::new();
        for (dim, rows) in ["day", "model", "project", "tool"].into_iter().zip(by) {
            let row = |(key, b): (String, Bucket)| {
                let [input, cache_write, cache_read, output] = b.fields.map(total);
                json!({"input": input, "cache_write": cache_write, "cache_read": cache_read,
                       "output": output, "reasoning": total(b.reasoning), "total": total(b.total),
                       "cost": b.cost, "priced": !b.unpriced, "calls": b.calls, "key": key,
                       "session_count": b.sessions.len()})
            };
            out.insert(dim.into(), rows.into_iter().map(row).collect());
        }
        let hours = |h: Vec<Option<Num>>| h.into_iter().map(total).collect();
        let tops =
            |[model, project]: [Sums; 2]| json!({"model": top(model), "project": top(project)});
        out.insert(
            "daily_tool".into(),
            object(daily_tool, |t| object(t, total)),
        );
        let costs = object(daily_cost, |t| object(t, |v| json!(round_to(v, 4))));
        out.insert("daily_tool_cost".into(), costs);
        out.insert(
            "daily_tool_unpriced".into(),
            object(daily_unpriced, |t| json!(t)),
        );
        out.insert("day_hours".into(), object(day_hours, |t| object(t, hours)));
        out.insert("year_top".into(), object(year_top, tops));
        // activity[kind]["name|tool"][day] = count; kinds: tool, agent, skill, mcp, command
        let (mut activity, mut longest) = (Map::new(), Map::new());
        for e in events.map_or(Ok(Vec::new()), iter)? {
            if *item(&e, "kind")? == "turn" {
                let (tool, ms) = (key_text(item(&e, "tool")?)?, num(item(&e, "ms")?)?);
                let best = longest.get(&tool).map_or(Ok(ZERO), num)?;
                longest.insert(tool, value(Num::max(&[best, ms])));
                continue;
            }
            let kind = key_text(&e["kind"])?;
            let name = format!(
                "{}|{}",
                str_of(item(&e, "name")?),
                str_of(item(&e, "tool")?)
            );
            let day = key_text(item(&e, "day")?)?;
            let slot = &mut activity.entry(kind).or_insert_with(|| json!({}))[name.as_str()][day];
            *slot = json!(slot.as_i64().unwrap_or(0) + 1);
        }
        out.insert("longest_turn_ms".into(), Value::Object(longest));
        out.insert("activity".into(), Value::Object(activity));
        Ok(Value::Object(out))
    }

    /// `utc_now_iso`
    pub fn utc_now_iso() -> String {
        chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S.000Z")
            .to_string()
    }

    /// `get_json`: `(body, None)` or `(None, error message)`.
    pub fn get_json(url: &str, headers: &[(&str, String)], app: &str) -> (Value, Value) {
        let wait = Some(Duration::from_secs(15));
        let config = ureq::Agent::config_builder()
            .timeout_resolve(wait)
            .timeout_connect(wait);
        let config = config.timeout_per_read(wait).timeout_per_write(wait);
        let mut request = ureq::Agent::from(config.build()).get(url);
        for (name, value) in headers {
            request = request.header(*name, value.as_str());
        }
        let error = match request.call() {
            Ok(mut response) => {
                let body = response.body_mut().read_to_vec().ok();
                match body.and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()) {
                    Some(body) => return (body, Value::Null),
                    None => "live check failed (offline?)".to_string(),
                }
            }
            Err(ureq::Error::StatusCode(401 | 403)) => {
                format!("{app} login expired, open {app} to renew it")
            }
            Err(ureq::Error::StatusCode(code)) => format!("live check failed (HTTP {code})"),
            Err(_) => "live check failed (offline?)".into(),
        };
        (Value::Null, json!(error))
    }

    /// The reading in a Codex usage response: `(reading, None)` or `(None, why not)`.
    pub fn codex_usage(body: &Value) -> Reading {
        if !body.is_object() {
            return Ok((
                Value::Null,
                json!("Codex returned an unrecognized usage response"),
            ));
        }
        let mut buckets = vec![(json!("codex"), &body["rate_limit"])];
        for extra in body["additional_rate_limits"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if extra.is_object() {
                let other = json!("Additional limit");
                let name = or(&extra["limit_name"], or(&extra["metered_feature"], &other));
                buckets.push((name.clone(), or(&extra["rate_limit"], extra)));
            }
        }
        if truthy(&body["code_review_rate_limit"]) {
            buckets.push((json!("Code review"), &body["code_review_rate_limit"]));
        }
        let mut windows = Vec::new();
        for (name, bucket) in buckets {
            for w in [
                sub(bucket, "primary_window")?,
                sub(bucket, "secondary_window")?,
            ] {
                if w.is_object() && !w["used_percent"].is_null() {
                    let minutes = match num_or_zero(&w["limit_window_seconds"])? {
                        Num::Int(s) => Num::Int(s.div_euclid(60)),
                        Num::Float(s) => Num::Float(float_floor_div(s, 60.0)),
                    };
                    windows.push(limit_window(&name, minutes, w, "reset_at")?);
                }
            }
        }
        if windows.is_empty() {
            let error = "Codex returned no usage windows; showing the last known reading";
            return Ok((Value::Null, json!(error)));
        }
        let plan = pretty_plan(body["plan_type"].clone())?;
        let details = codex_details(body.clone())?;
        Ok((
            json!({"plan": plan, "windows": windows, "details": details}),
            Value::Null,
        ))
    }

    /// The reading in a Claude usage response, for the saved login `creds`.
    pub fn claude_usage(body: &Value, creds: &Value) -> Result<Value, String> {
        let mut windows = Vec::new();
        let window = |label: Value, w: &Value, used: &str| -> Result<Value, String> {
            let used = or(at(w, used)?, &json!(0)).clone();
            Ok(json!({"label": label, "used_percent": used, "resets_at": epoch(&w["resets_at"])}))
        };
        for lim in at(body, "limits")?.as_array().into_iter().flatten() {
            let model = sub(sub(sub(lim, "scope")?, "model")?, "display_name")?;
            let label = match at(lim, "kind")?.as_str() {
                Some("session") => json!("5-hour session"),
                Some("weekly_all") => json!("7-day, all models"),
                _ if truthy(model) => json!(format!("7-day, {}", str_of(model))),
                _ => lim.get("kind").cloned().unwrap_or_else(|| json!("?")),
            };
            windows.push(window(label, lim, "percent")?);
        }
        // older response shape
        for (key, label) in [
            ("five_hour", "5-hour session"),
            ("seven_day", "7-day, all models"),
        ] {
            if truthy(&body[key]) && body["limits"].as_array().is_none_or(Vec::is_empty) {
                windows.push(window(json!(label), &body[key], "utilization")?);
            }
        }
        let mut breakdown = Vec::new();
        let rows = sub(&body["seven_day_breakdown"], "rows")?;
        for r in rows.as_array().into_iter().flatten() {
            let (name, percent) = (&r["display_name"], &r["percent"]);
            breakdown.push(json!({"key": at(r, "key")?, "name": name, "percent": percent}));
        }
        let used = sub(&body["spend"], "used")?;
        let amount = sub(used, "amount_minor")?;
        let mut spend = Value::Null;
        let whole = amount.is_i64() || amount.is_u64() || amount.is_boolean();
        if truthy(&body["spend"]["enabled"]) && whole {
            let amount = num(amount)?.f() / 10_f64.powf(num_or_zero(&used["exponent"])?.f());
            spend = json!({"used": amount, "currency": text_or(&used["currency"], "")});
        }
        let tier = text_or(&creds["rateLimitTier"], "").replace("default_claude_", "");
        let plan = pretty_plan(or(&json!(tier), &creds["subscriptionType"]).clone())?;
        Ok(json!({"plan": plan, "windows": windows, "breakdown": breakdown, "spend": spend}))
    }

    /// `working_now` from the sessions `working_details` lists.
    pub fn count_tools(sessions: &[Value]) -> Value {
        let count = |tool: &str| sessions.iter().filter(|s| s["tool"] == tool).count();
        json!({"Claude Code": count("Claude Code"), "Codex": count("Codex")})
    }

    /// The page `write_dashboard` writes: `template` with the data in place of its marker.
    pub fn render_dashboard(template: &str, data: &Value) -> String {
        let page = template.replace("\r\n", "\n").replace('\r', "\n");
        let page = page.replace(
            "/*__DATA__*/null",
            &dumps_ascii(data).replace('<', "\\u003c"),
        );
        if cfg!(windows) {
            page.replace('\n', "\r\n")
        } else {
            page
        }
    }

    /// `write_dashboard`: the bundled template with the data, written to `out_path`.
    pub fn write_dashboard(data: &Value, out_path: &Path) -> std::io::Result<()> {
        std::fs::write(out_path, render_dashboard(TEMPLATE, data))
    }

    /// The rows of a query with at most one argument; `None` for a database error.
    fn query(db: &rusqlite::Connection, sql: &str, arg: Option<&Value>) -> Option<Vec<Vec<Value>>> {
        use rusqlite::types::Value as Sql;
        let arg = arg.map(|v| match v {
            Value::String(text) => Sql::Text(text.clone()),
            Value::Number(n) => n
                .as_i64()
                .map_or(Sql::Real(n.as_f64().unwrap_or_default()), Sql::Integer),
            _ => Sql::Null,
        });
        let mut statement = db.prepare(sql).ok()?;
        let columns = statement.column_count();
        let mut rows = statement.query(rusqlite::params_from_iter(arg)).ok()?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().ok()? {
            let cell = |i| match row.get(i).ok()? {
                Sql::Null => Some(Value::Null),
                Sql::Integer(i) => Some(json!(i)),
                Sql::Real(x) => Some(json!(x)),
                Sql::Text(text) => Some(json!(text)),
                Sql::Blob(bytes) => Some(json!(String::from_utf8_lossy(&bytes))),
            };
            out.push((0..columns).map(cell).collect::<Option<_>>()?);
        }
        Some(out)
    }

    /// The saved login under `key` of a login file, and its token; `None` without one.
    fn login<'a>(
        saved: &'a Option<Value>,
        key: &str,
        token: &str,
    ) -> Option<(&'a Value, &'a Value)> {
        let login = saved.as_ref()?.as_object()?.get(key)?;
        Some((login, login.as_object()?.get(token)?))
    }

    impl Dirs {
        /// The folders of this user, as `tracker.py` finds them.
        pub fn from_env() -> Self {
            let home = |var: &str, folder: &str| {
                let default = || std::env::home_dir().unwrap_or_default().join(folder);
                std::env::var_os(var).map_or_else(default, PathBuf::from)
            };
            let exe = std::env::current_exe().ok();
            let here = exe
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .unwrap_or_default();
            let (claude, codex) = (
                home("CLAUDE_CONFIG_DIR", ".claude"),
                home("CODEX_HOME", ".codex"),
            );
            Self {
                claude,
                codex,
                here,
                store: limits_watch::store(),
            }
        }

        /// `scan_commands`: slash commands typed in Claude Code and Codex, by name only.
        pub fn scan_commands(
            &self,
            events: &mut Vec<Value>,
            warnings: &mut Vec<String>,
        ) -> Result<(), String> {
            let claude = ("Claude Code", &self.claude, "display", "timestamp", 1000.0);
            for (tool, home, text, time, per_second) in
                [claude, ("Codex", &self.codex, "text", "ts", 1.0)]
            {
                let path = home.join("history.jsonl");
                if !path.exists() {
                    continue;
                }
                iter_jsonl(&path, warnings, &mut |row| {
                    let text = text_or(&row[text], "");
                    let word = split_ws(&text).first().copied().filter(|w| is_command(w));
                    let day = num_or_zero(&row[time])
                        .ok()
                        .and_then(|t| day_of(t.f() / per_second));
                    if let (Some(word), Some(day)) = (word, day) {
                        events.push(event(tool, &json!(day), "command", json!(word)));
                    }
                    Ok(true)
                })?;
            }
            Ok(())
        }

        /// `codex_titles`: thread names by session id.
        pub fn codex_titles(&self) -> Result<Value, String> {
            let (mut titles, path) = (Map::new(), self.codex.join("session_index.jsonl"));
            if path.exists() {
                iter_jsonl(&path, &mut Vec::new(), &mut |row| {
                    if truthy(&row["id"]) && truthy(&row["thread_name"]) {
                        hash_key(&row["id"])?;
                        titles.insert(str_of(&row["id"]), row["thread_name"].clone());
                    }
                    Ok(true)
                })?;
            }
            Ok(Value::Object(titles))
        }

        /// `codex_db`: Codex's newest versioned database, e.g. `state_5.sqlite`, opened read-only.
        fn codex_db(&self, prefix: &str) -> Option<rusqlite::Connection> {
            let version = |path: &PathBuf| {
                let name = name_of(path);
                let tail = stem(&name).get(prefix.len() + 1..).unwrap_or("");
                let whole = !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit());
                if whole {
                    tail.parse::<i128>().unwrap_or(i128::MAX)
                } else {
                    -1
                }
            };
            let named = |p: &PathBuf| {
                let name = name_of(p);
                name.starts_with(&format!("{prefix}_")) && name.ends_with(".sqlite")
            };
            let entries = std::fs::read_dir(&self.codex)
                .ok()?
                .flatten()
                .map(|e| e.path());
            let mut paths: Vec<PathBuf> = entries.filter(named).collect();
            paths.sort_by_key(version);
            let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY;
            let db = rusqlite::Connection::open_with_flags(paths.last()?, flags).ok()?;
            db.busy_timeout(Duration::from_secs(1)).ok()?;
            Some(db)
        }

        /// `codex_db_working`: Codex threads with a turn in progress, from Codex's own databases,
        /// and the ids of every thread those databases track.
        pub fn codex_db_working(&self, now: f64) -> Result<(Vec<Value>, Vec<Value>), String> {
            let dbs = (self.codex_db("state"), self.codex_db("thread_history"));
            let (Some(state), Some(history)) = dbs else {
                return Ok(Default::default());
            };
            // `Err(None)` is a database error (mid-migration or locked): fall back to the
            // rollout logs alone.
            let read = || -> Result<(Vec<Value>, Vec<Value>), Option<String>> {
                let first = |rows: Vec<Vec<Value>>| rows.into_iter().map(|mut r| r.swap_remove(0));
                let tracked = "SELECT id FROM threads WHERE history_mode != 'legacy'";
                let covered = first(query(&state, tracked, None).ok_or(None)?).collect();
                let running =
                    "SELECT DISTINCT thread_id FROM thread_turns WHERE status = 'inProgress'";
                let running = query(&history, running, None).ok_or(None)?;
                let edges = "SELECT child_thread_id, parent_thread_id FROM thread_spawn_edges";
                let edges = query(&state, edges, None).ok_or(None)?;
                let mut sessions = Vec::new();
                for tid in first(running) {
                    let thread = "SELECT model, reasoning_effort, cwd, name, title, thread_source, agent_nickname, \
                                  attached_thread_parent_id, updated_at_ms FROM threads WHERE id = ?";
                    let found = query(&state, thread, Some(&tid)).ok_or(None)?;
                    let Some(row) = found.first() else { continue };
                    let last = "SELECT MAX(created_at_ms) FROM thread_items WHERE thread_id = ?";
                    let last = query(&history, last, Some(&tid)).ok_or(None)?;
                    let newest = Num::max(&[num_or_zero(&row[8])?, num_or_zero(&last[0][0])?]);
                    let age = int_of(now - newest.f() / 1000.0)?;
                    if age > CODEX_STALE_TURN {
                        continue;
                    }
                    let prompt = "SELECT item_json FROM thread_items WHERE thread_id = ? AND item_type = 'userMessage' \
                                  ORDER BY rollout_ordinal DESC LIMIT 1";
                    let mut task = Value::Null;
                    if let Some(found) = query(&history, prompt, Some(&tid)).ok_or(None)?.first() {
                        let odd = || String::from("TypeError: the JSON object must be str");
                        let item =
                            serde_json::from_str::<Value>(found[0].as_str().ok_or_else(odd)?);
                        let text =
                            joined_text(item.as_ref().map_or(Ok(&NULL), |i| at(i, "content"))?)?;
                        if !text.chars().all(is_space) {
                            task = short_text_of(&json!(text), 90);
                        }
                    }
                    let mut project = Value::Null;
                    set_project(&mut project, &row[2])?;
                    let parent = edges
                        .iter()
                        .rev()
                        .find(|edge| edge[0] == tid)
                        .map_or(&NULL, |edge| &edge[1]);
                    sessions.push(json!({"tool": "Codex", "model": row[0], "effort": row[1], "id": tid,
                                         "project": project, "title": or(&row[3], or(&row[4], &NULL)),
                                         "task": task, "age": age.max(0), "parent": or(parent, &row[7]),
                                         "nickname": row[6], "source": row[5]}));
                }
                Ok((sessions, covered))
            };
            read().or_else(|error| error.map_or(Ok(Default::default()), Err))
        }

        /// `working_details`: what each working session is doing: model, effort, project, title,
        /// latest prompt, and its subagents. `now` is the clock's when `None` or zero.
        pub fn working_details(&self, window: i64, now: Option<f64>) -> Result<Vec<Value>, String> {
            let now = now.filter(|t| *t != 0.0).unwrap_or_else(clock);
            let (mut sessions, mut subagents, mut titles) = (Map::new(), Map::new(), None);
            let (mut codex_running, mut covered) = (Vec::new(), Vec::new());
            if self.codex.exists() {
                (codex_running, covered) = self.codex_db_working(now)?;
            }
            let lower = |info: &mut Value, age: i64| {
                info["age"] = json!(info["age"].as_i64().map_or(age, |a| a.min(age)));
            };
            let push = |subs: &mut Map<String, Value>, key: String, sub: Value| {
                if let Value::Array(list) = subs.entry(key).or_insert_with(|| json!([])) {
                    list.push(sub);
                }
            };
            for (claude, root) in [
                (true, self.claude.join("projects")),
                (false, self.codex.join("sessions")),
            ] {
                for path in rglob(&root, &is_log) {
                    let Ok(written) = std::fs::metadata(&path).and_then(|m| m.modified()) else {
                        continue;
                    };
                    let written = written.duration_since(UNIX_EPOCH).unwrap_or_default();
                    let age = int_of(
                        now - (written.as_secs() as f64 + 1e-9 * f64::from(written.subsec_nanos())),
                    )?;
                    if age > window {
                        continue;
                    }
                    let folder = path.parent().unwrap_or(&root);
                    if claude && name_of(folder) == "subagents" {
                        let parent = with_suffix(folder.parent().unwrap_or(&root), ".jsonl");
                        let key = parent.to_string_lossy().into_owned();
                        let meta = read_json(&with_suffix(&path, ".meta.json"))
                            .unwrap_or_else(|| json!({}));
                        let info = claude_session_info(&path)?;
                        let kind = format!("{} agent", text_or(at(&meta, "agentType")?, "sub"));
                        let about = short_text_of(&meta["description"], 70);
                        let sub = json!({"type": kind, "description": about, "model": info["model"],
                                         "effort": info["effort"], "age": age});
                        push(&mut subagents, key.clone(), sub);
                        if !sessions.contains_key(&key) {
                            // parent idle while its subagent works: describe it from its own log
                            sessions.insert(key.clone(), claude_session_info(&parent)?);
                        }
                        lower(&mut sessions[&key], age);
                    } else if claude {
                        let info = claude_session_info(&path)?;
                        let key = path.to_string_lossy().into_owned();
                        lower(sessions.entry(key).or_insert(info), age);
                    } else {
                        let name = name_of(&path);
                        let id = stem(&name).chars().rev().take(36).collect::<Vec<_>>();
                        if covered.contains(&json!(id.into_iter().rev().collect::<String>())) {
                            continue; // its database says whether it's running (below)
                        }
                        let titles = match &mut titles {
                            Some(titles) => titles,
                            none => none.insert(self.codex_titles()?),
                        };
                        let mut info = codex_session_info(&path, titles)?;
                        info["age"] = json!(age);
                        codex_running.push(info);
                    }
                }
            }
            for (at, info) in codex_running.into_iter().enumerate() {
                let (parent, id) = (&info["parent"], &info["id"]);
                if truthy(parent) {
                    let review = info["source"] == "guardian_review";
                    let kind = if review { "review agent" } else { "subagent" };
                    let named = truthy(&info["nickname"])
                        .then(|| format!("{} ({kind})", str_of(&info["nickname"])));
                    let sub = json!({"type": named.unwrap_or_else(|| kind.into()), "description": info["task"],
                                     "model": info["model"], "effort": info["effort"], "age": info["age"],
                                     "project": info["project"]});
                    push(&mut subagents, format!("codex:{}", text(parent)?), sub);
                } else if truthy(id) {
                    let key = format!("codex:{}", text(id)?);
                    sessions.insert(key, info);
                } else {
                    sessions.insert(format!("codex:#{at}"), info);
                }
            }
            for (key, subs) in subagents {
                let mut subs = subs.as_array().cloned().unwrap_or_default();
                if !sessions.contains_key(&key) {
                    // a Codex subagent whose parent thread is quiet
                    let title = titles
                        .as_ref()
                        .and_then(|t| t.get(key.get(6..).unwrap_or("")));
                    let age = subs.iter().filter_map(|s| s["age"].as_i64()).min();
                    let quiet = json!({"tool": "Codex", "model": null, "effort": null,
                                       "project": subs[0].get("project"), "title": title, "task": null, "age": age});
                    sessions.insert(key.clone(), quiet);
                }
                subs.sort_by_key(|s| s["age"].as_i64());
                sessions[&key]["subagents"] = json!(subs);
            }
            let mut out = Vec::new();
            for (_, mut info) in sessions {
                if let Value::Object(map) = &mut info {
                    map.entry("subagents").or_insert_with(|| json!([]));
                    map.retain(|k, _| {
                        !["id", "parent", "nickname", "source"].contains(&k.as_str())
                    });
                }
                out.push(info);
            }
            out.sort_by_key(|info| info["age"].as_i64());
            Ok(out)
        }

        /// `working_now`: working sessions per tool (a subagent counts as part of its parent).
        pub fn working_now(&self, window: i64, now: Option<f64>) -> Result<Value, String> {
            Ok(count_tools(&self.working_details(window, now)?))
        }

        /// `scan_claude`: one record per API response; Claude Code writes a line per content
        /// block with the same usage. Tool calls (each counted once by its id) and turn lengths
        /// are appended to `events` when given.
        pub fn scan_claude(
            &self,
            mut events: Option<&mut Vec<Value>>,
            warnings: &mut Vec<String>,
        ) -> Result<Vec<Value>, String> {
            let root = self.claude.join("projects");
            let (mut records, mut seen, mut calls) = (Vec::new(), HashSet::new(), HashSet::new());
            let (zero, tool) = (json!(0), "Claude Code");
            for path in rglob(&root, &is_log) {
                let project = path.strip_prefix(&root).ok().and_then(|p| p.iter().next());
                let project = json!(project.unwrap_or_default().to_string_lossy());
                let (folder, name) = (path.parent().unwrap_or(&root), name_of(&path));
                let sub = name_of(folder) == "subagents";
                let session = if sub {
                    name_of(folder.parent().unwrap_or(&root))
                } else {
                    stem(&name).to_string()
                };
                iter_jsonl(&path, warnings, &mut |row| {
                    let (stamp, msg) = (&row["timestamp"], &row["message"]);
                    let turn = row["type"] == "system" && row["subtype"] == "turn_duration";
                    if let (Some(events), true) = (events.as_deref_mut(), turn) {
                        let ms = &row["durationMs"];
                        if (ms.is_number() || ms.is_boolean())
                            && calls.insert(hash_key(&row["uuid"])?)
                        {
                            events.push(json!({"tool": tool, "day": local_day_of(stamp)?, "kind": "turn", "ms": ms}));
                        }
                        return Ok(true);
                    }
                    let blocks = msg
                        .get("content")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten();
                    for block in blocks.filter(|b| b.is_object() && b["type"] == "tool_use") {
                        let Some(events) = events.as_deref_mut() else {
                            break;
                        };
                        if calls.insert(hash_key(&block["id"])?) {
                            let day = local_day_of(stamp)?;
                            let args = if block["input"].is_object() {
                                &block["input"]
                            } else {
                                &json!({})
                            };
                            let found =
                                tool_events(&text_or(&block["name"], "?"), args)?.into_iter();
                            let found = found.filter(|_| truthy(&day));
                            events.extend(found.map(|(kind, name)| event(tool, &day, kind, name)));
                        }
                    }
                    let model = or(&msg["model"], &json!("unknown")).clone();
                    if !msg.is_object() || !msg["usage"].is_object() || model == "<synthetic>" {
                        return Ok(true);
                    }
                    let key = format!("{} {}", hash_key(&msg["id"])?, hash_key(&row["requestId"])?);
                    if truthy(&msg["id"]) && seen.contains(&key) {
                        return Ok(true);
                    }
                    seen.insert(key);
                    let (u, [day, hour, dow], cwd) =
                        (&msg["usage"], local_time_of(stamp), &row["cwd"]);
                    let project = match truthy(cwd).then(|| path_text(cwd)).transpose()? {
                        None => project.clone(),
                        Some(cwd) if cfg!(windows) => json!(windows_name(cwd)),
                        Some(cwd) => json!(posix_name(cwd)),
                    };
                    records.push(json!({
                        "tool": tool, "day": day, "hour": hour, "dow": dow, "model": model, "project": project,
                        "session": session, "input": or(&u["input_tokens"], &zero),
                        "cache_write": or(&u["cache_creation_input_tokens"], &zero),
                        "cache_read": or(&u["cache_read_input_tokens"], &zero),
                        "output": or(&u["output_tokens"], &zero),
                    }));
                    Ok(true)
                })?;
            }
            Ok(records)
        }

        /// `scan_codex`: usage counted once from the running totals, including archives and
        /// excluding inherited fork history, and the newest rate limits per limit id (`null`
        /// without any). Tool calls are appended to `events` when given.
        pub fn scan_codex(
            &self,
            mut events: Option<&mut Vec<Value>>,
            warnings: &mut Vec<String>,
        ) -> Result<(Vec<Value>, Value), String> {
            let (mut records, mut limits_by_id, mut seen) =
                (Vec::new(), Map::new(), HashSet::new());
            let (mut calls, mut session_totals) =
                (HashSet::new(), HashMap::<String, (f64, Value)>::new());
            let logs = |name: &str| name.ends_with(".jsonl") || name.ends_with(".jsonl.zst");
            let mut paths: Vec<(String, String, PathBuf)> = Vec::new();
            for path in ["sessions", "archived_sessions"]
                .iter()
                .flat_map(|r| rglob(&self.codex.join(r), &logs))
            {
                let name = name_of(&path);
                let sorted = name.strip_suffix(".zst").unwrap_or(&name).to_string();
                paths.push((sorted, path.to_string_lossy().into_owned(), path));
            }
            paths.sort();
            for (name, _, path) in paths {
                let (mut model, mut project, mut prev) =
                    (json!("unknown"), json!("unknown"), None::<Value>);
                let (mut session, mut fork_time) = (json!(name.split('.').next()), None::<f64>);
                iter_jsonl(&path, warnings, &mut |row| {
                    let (payload, kind, stamp) = (&row["payload"], &row["type"], &row["timestamp"]);
                    if truthy(payload) && !payload.is_object() {
                        return Ok(true);
                    }
                    let inherited =
                        |fork: Option<f64>| fork.is_some_and(|f| epoch_or_zero(stamp) < f);
                    let called =
                        payload["type"] == "function_call" || payload["type"] == "custom_tool_call";
                    if kind == "session_meta" {
                        session = or(&payload["id"], &session).clone();
                        if truthy(&payload["forked_from_id"]) {
                            fork_time = epoch(or(&payload["timestamp"], stamp));
                        }
                        set_project(&mut project, &payload["cwd"])?;
                    } else if kind == "turn_context" {
                        model = or(&payload["model"], &model).clone();
                        set_project(&mut project, &payload["cwd"])?;
                    } else if let (Some(events), true) =
                        (events.as_deref_mut(), kind == "response_item" && called)
                    {
                        let call_id = or(&payload["call_id"], &payload["id"]);
                        let day = local_day_of(stamp)?;
                        let call = format!("{} {}", hash_key(&session)?, hash_key(call_id)?);
                        if truthy(&day)
                            && !inherited(fork_time)
                            && (call_id.is_null() || !calls.contains(&call))
                        {
                            calls.insert(call);
                            let name = text_or(&payload["name"], "?");
                            let namespace = text_or(&payload["namespace"], "");
                            let mut found = vec![("tool", name.clone())];
                            if let Some(server) = namespace.strip_prefix("mcp__") {
                                let server = server.trim_matches('_');
                                found = vec![
                                    ("tool", format!("MCP: {server}")),
                                    ("mcp", server.to_string()),
                                ];
                            } else if name == "spawn_agent" {
                                found.push(("agent", "Codex subagent".into()));
                            }
                            events.extend(
                                found
                                    .into_iter()
                                    .map(|(kind, name)| event("Codex", &day, kind, json!(name))),
                            );
                        }
                    } else if kind == "event_msg" && payload["type"] == "token_count" {
                        let limits = &payload["rate_limits"];
                        let has_window =
                            truthy(sub(limits, "primary")?) || truthy(&limits["secondary"]);
                        let limit_id = text_or(&limits["limit_id"], "codex");
                        let newer = |prior: &Value| {
                            epoch_or_zero(stamp) >= epoch_or_zero(&prior["timestamp"])
                        };
                        if has_window && limits_by_id.get(&limit_id).is_none_or(newer) {
                            let mut bucket = limits.clone();
                            bucket["timestamp"] =
                                row.get("timestamp").cloned().unwrap_or_else(|| json!(""));
                            limits_by_id.insert(limit_id, bucket);
                        }
                        let (total, last) = (
                            sub(&payload["info"], "total_token_usage")?,
                            &payload["info"]["last_token_usage"],
                        );
                        if !truthy(total) {
                            return Ok(true);
                        }
                        if inherited(fork_time) {
                            prev = Some(total.clone());
                            return Ok(true);
                        }
                        let who = hash_key(&session)?;
                        let mut sorted: Vec<(&String, &Value)> =
                            total.as_object().into_iter().flatten().collect();
                        sorted.sort_by_key(|pair| pair.0);
                        if !seen.insert(format!("{who} {stamp} {}", json!(sorted))) {
                            return Ok(true);
                        }
                        let event_time = epoch_or_zero(stamp);
                        if let Some((previous_time, previous_total)) = session_totals.get(&who) {
                            if event_time < *previous_time {
                                return Ok(true); // an older copy of an already scanned session
                            }
                            prev = Some(previous_total.clone());
                        }
                        let fell =
                            |prev: &Value, k| Ok::<_, String>(count(total, k)? < count(prev, k)?);
                        let delta = match &prev {
                            // A normal session's first snapshot can include multiple requests.
                            // A fork with no copied baseline can include its parent's usage.
                            None if fork_time.is_some() => or(last, &json!({})).clone(),
                            None => total.clone(),
                            Some(prev)
                                if fell(prev, "input_tokens")? || fell(prev, "output_tokens")? =>
                            {
                                or(last, total).clone()
                            }
                            Some(prev) => {
                                let mut delta = Map::new();
                                for k in total.as_object().into_iter().flat_map(Map::keys) {
                                    let grown =
                                        Num::max(&[count(total, k)? - count(prev, k)?, ZERO]);
                                    delta.insert(k.clone(), value(grown));
                                }
                                Value::Object(delta)
                            }
                        };
                        prev = Some(total.clone());
                        session_totals.insert(who, (event_time, total.clone()));
                        let part = |key: &str| {
                            at(&delta, key).map(|v| delta.get(key).map_or(json!(0), |_| v.clone()))
                        };
                        if !truthy(&part("input_tokens")?) && !truthy(&part("output_tokens")?) {
                            return Ok(true); // repeated snapshot, no new usage
                        }
                        let input = count(&delta, "input_tokens")?;
                        let cached = Num::min(&[
                            Num::max(&[count(&delta, "cached_input_tokens")?, ZERO]),
                            input,
                        ]);
                        let [day, hour, dow] = local_time_of(stamp);
                        records.push(json!({
                            "tool": "Codex", "day": day, "hour": hour, "dow": dow, "model": model, "project": project,
                            "session": session, "input": value(Num::max(&[input - cached, ZERO])),
                            "cache_write": part("cache_write_input_tokens")?, "cache_read": value(cached),
                            "output": part("output_tokens")?, "reasoning": part("reasoning_output_tokens")?,
                        }));
                    }
                    Ok(true)
                })?;
            }
            let limits = if limits_by_id.is_empty() {
                Value::Null
            } else {
                Value::Object(limits_by_id)
            };
            Ok((records, limits))
        }

        /// `codex_live` with the request replaced: `(reading, None)` or `(None, why not)`.
        pub fn codex_live_with(&self, fetch: Fetch) -> Reading {
            let saved = read_json(&self.codex.join("auth.json"));
            let Some((tokens, token)) = login(&saved, "tokens", "access_token") else {
                return Ok((Value::Null, json!("no Codex login found")));
            };
            let headers = [
                ("Authorization", format!("Bearer {}", text(token)?)),
                ("ChatGPT-Account-Id", text_or(&tokens["account_id"], "")),
                ("User-Agent", "codex_cli_rs".into()),
                ("Accept", "application/json".into()),
            ];
            match fetch(CODEX_USAGE_URL, &headers, "Codex") {
                (body, error) if !truthy(&error) => codex_usage(&body),
                (_, error) => Ok((Value::Null, error)),
            }
        }

        /// `claude_live` at the time `now`, with the request replaced.
        pub fn claude_live_with(&self, now: f64, fetch: Fetch) -> Reading {
            let saved = read_json(&self.claude.join(".credentials.json"));
            let Some((creds, token)) = login(&saved, "claudeAiOauth", "accessToken") else {
                return Ok((Value::Null, json!("no Claude Code login found")));
            };
            if num_or_zero(&creds["expiresAt"])?.f() / 1000.0 < now {
                let error = "Claude Code login expired, open Claude Code to renew it";
                return Ok((Value::Null, json!(error)));
            }
            let headers = [
                ("Authorization", format!("Bearer {}", text(token)?)),
                ("anthropic-beta", "oauth-2025-04-20".into()),
                ("User-Agent", "claude-cli".into()),
                ("Accept", "application/json".into()),
            ];
            match fetch(CLAUDE_USAGE_URL, &headers, "Claude Code") {
                (body, error) if !truthy(&error) => Ok((claude_usage(&body, creds)?, Value::Null)),
                (_, error) => Ok((Value::Null, error)),
            }
        }

        /// `plan_limits` with the live checks replaced: a live reading is stamped `as_of`.
        pub fn plan_limits_with(
            &self,
            snapshot: &Value,
            log: Log,
            check: Check,
            as_of: &str,
        ) -> Result<Value, String> {
            let path = self.store.join("limits_cache.json");
            let mut cache = read_json(&path)
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({}));
            let mut entries = Vec::new();
            let logged = codex_log_limits(snapshot.clone())?;
            for (service, fallback) in [("Claude Code", Value::Null), ("Codex", logged)] {
                log(&format!("Checking live {service} limits ..."));
                let (live, error) = check(service)?;
                log(&format!("  {}", text_or(&error, "ok")));
                let mut entry = json!({"windows": []});
                if truthy(&live) {
                    entry = live;
                    entry["as_of"] = json!(as_of);
                    entry["source"] = json!("live");
                    cache[service] = entry.clone();
                } else {
                    let mut cached = cache[service].clone();
                    if cached.is_object() {
                        cached["source"] = json!("cached");
                    }
                    let mut newest = f64::NEG_INFINITY;
                    for stale in [cached, fallback]
                        .into_iter()
                        .filter(|e| truthy(e) && e.is_object())
                    {
                        let read_at = epoch_or_zero(&stale["as_of"]);
                        if read_at > newest {
                            (entry, newest) = (stale, read_at);
                        }
                    }
                }
                entry["service"] = json!(service);
                entry["error"] = error;
                entries.push(entry);
            }
            // Like the original, a cache that cannot be written is not an error.
            let _ = std::fs::create_dir_all(&self.store)
                .and_then(|()| std::fs::write(&path, dumps(&cache)));
            Ok(Value::Array(entries))
        }

        /// `plan_limits`: one entry per service: live if possible, else the newest of the last
        /// live reading and (Codex) the logs.
        pub fn plan_limits(&self, codex_snapshot: &Value, log: Log) -> Result<Value, String> {
            let mut check = |service: &str| match service {
                "Codex" => self.codex_live_with(&get_json),
                _ => self.claude_live_with(clock(), &get_json),
            };
            self.plan_limits_with(codex_snapshot, log, &mut check, &utc_now_iso())
        }

        /// `load_pricing`: the priced models of `pricing.json`, empty without the file.
        pub fn load_pricing(&self) -> Result<Value, String> {
            let path = self.here.join("pricing.json");
            if !path.exists() {
                return Ok(json!({}));
            }
            let text = std::fs::read_to_string(&path).map_err(|e| format!("OSError: {e}"))?;
            let data: Value =
                serde_json::from_str(&text).map_err(|e| format!("ValueError: {e}"))?;
            let mut priced = Map::new();
            for (model, rate) in data
                .get("models")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                at(rate, "")?;
                if FIELDS.iter().any(|f| truthy(&rate[*f])) {
                    priced.insert(model.clone(), rate.clone());
                }
            }
            Ok(Value::Object(priced))
        }

        /// `collect`: the data the dashboard and the widget show. `api_usage` and `openai` stand
        /// for `api_billing.api_usage` and `openai_analytics.collect`.
        pub fn collect(&self, log: Log, api_usage: Part, openai: Part) -> Result<Value, String> {
            let (mut warnings, mut events) = (Vec::new(), Vec::new());
            log("Scanning Claude Code logs ...");
            let mut records = self.scan_claude(Some(&mut events), &mut warnings)?;
            log(&format!("  {} API responses", records.len()));
            log("Scanning Codex logs ...");
            let (codex, limits) = self.scan_codex(Some(&mut events), &mut warnings)?;
            self.scan_commands(&mut events, &mut warnings)?;
            log(&format!("  {} turns with usage", codex.len()));
            records.extend(codex);
            let pricing = self.load_pricing()?;
            let mut data = aggregate(&json!(records), &pricing, Some(&json!(events)))?;
            data["plan_limits"] = self.plan_limits(&limits, &mut *log)?;
            data["api"] = api_usage(&mut *log);
            data["openai_analytics"] = openai(&mut *log);
            data["has_pricing"] = json!(truthy(&pricing));
            data["generated"] = json!(Local::now().format("%Y-%m-%d %H:%M").to_string());
            data["today"] = json!(Local::now().format("%Y-%m-%d").to_string());
            let now = chrono::Utc::now().timestamp();
            data["now_epoch"] = json!(now);
            let history = self.store.join("limits_history.json");
            limits_watch::record_and_project_at(
                &mut data["plan_limits"],
                limits_watch_rs::pynum::Num::Int(now),
                &history,
            )?;
            data["warnings"] = json!(warnings);
            Ok(data)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::native::*;
    use super::*;

    const T0: f64 = 1_795_000_000.0;
    const AT: &str = "2026-11-01T10:01:00Z";

    /// What the Python original gives: see `fixture/make_expected.py`.
    fn expected() -> Value {
        serde_json::from_str(include_str!("../fixture/expected.json")).unwrap_or_default()
    }

    /// The folders of a fresh, empty home.
    fn home(name: &str) -> Dirs {
        let root = std::env::temp_dir().join(format!("tracker-rs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (claude, codex, store) = (root.join("claude"), root.join("codex"), root.join("store"));
        Dirs {
            claude,
            codex,
            here: root,
            store,
        }
    }

    fn write_bytes(path: &Path, bytes: &[u8]) {
        let made = path.parent().map_or(Ok(()), std::fs::create_dir_all);
        assert!(made.and_then(|()| std::fs::write(path, bytes)).is_ok());
    }

    fn lines(rows: &[Value]) -> String {
        rows.iter().map(|row| format!("{row}\n")).collect()
    }

    fn write(path: &Path, rows: &[Value]) {
        write_bytes(path, lines(rows).as_bytes());
    }

    fn meta(session: &str, fork: Option<&str>) -> Value {
        let payload = json!({"id": session, "cwd": "/projects/demo", "forked_from_id": fork,
                             "timestamp": "2026-11-01T10:00:00Z"});
        json!({"type": "session_meta", "timestamp": "2026-11-01T10:00:00Z", "payload": payload})
    }

    fn usage(n: i64, last: i64, timestamp: &str) -> Value {
        let counts = |x: i64| {
            json!({"input_tokens": x * 10, "cached_input_tokens": x * 4, "output_tokens": x * 2,
                   "reasoning_output_tokens": x, "total_tokens": x * 12})
        };
        let info = json!({"total_token_usage": counts(n), "last_token_usage": counts(last)});
        json!({"type": "event_msg", "timestamp": timestamp,
               "payload": {"type": "token_count", "info": info}})
    }

    /// The records of a Codex home holding these session logs, and the warnings.
    fn codex_records(name: &str, logs: &[(&str, Vec<Value>)]) -> (Vec<Value>, Value, Vec<String>) {
        let (dirs, mut warnings) = (home(name), Vec::new());
        for (file, rows) in logs {
            match file.strip_suffix(".zst") {
                Some(_) => {
                    let packed = zstd::encode_all(lines(rows).as_bytes(), 0).unwrap_or_default();
                    write_bytes(&dirs.codex.join(file), &packed);
                }
                None => write(&dirs.codex.join(file), rows),
            }
        }
        let (records, limits) = dirs.scan_codex(None, &mut warnings).unwrap_or_default();
        (records, limits, warnings)
    }

    fn output(name: &str, logs: &[(&str, Vec<Value>)]) -> Option<i64> {
        codex_records(name, logs)
            .0
            .iter()
            .map(|r| r["output"].as_i64())
            .sum()
    }

    #[test]
    fn codex_usage_is_counted_once() {
        let first = codex_records(
            "first",
            &[("sessions/a.jsonl", vec![meta("s", None), usage(5, 2, AT)])],
        )
        .0;
        let r = &first[0];
        assert_eq!(
            json!([r["input"], r["cache_read"], r["output"], r["reasoning"]]),
            json!([30, 20, 10, 5])
        );
        assert_eq!(
            aggregate(&json!(first), &json!({}), None).map(|out| out["tool"][0]["total"].clone()),
            Ok(json!(60))
        );
        let repeats = vec![
            meta("s", None),
            usage(5, 5, AT),
            usage(5, 5, AT),
            usage(7, 7, AT),
            usage(2, 2, AT),
        ];
        assert_eq!(output("reset", &[("sessions/a.jsonl", repeats)]), Some(18));
        let rows = vec![meta("s", None), usage(5, 5, AT), usage(7, 7, AT)];
        let copies = [
            ("sessions/a.jsonl", rows.clone()),
            ("archived_sessions/a.jsonl.zst", rows),
        ];
        assert_eq!(output("archived", &copies), Some(14));
        let copied = usage(5, 5, "2026-11-01T09:59:59.999Z");
        let fork = vec![meta("child", Some("parent")), copied, usage(7, 7, AT)];
        assert_eq!(output("fork", &[("sessions/fork.jsonl", fork)]), Some(4));
        let resumed = vec![meta("s", None), usage(7, 7, "2026-11-01T10:02:00Z")];
        let files = [
            ("sessions/a.jsonl", vec![meta("s", None), usage(5, 5, AT)]),
            ("sessions/b.jsonl", resumed),
        ];
        assert_eq!(output("resumed", &files), Some(14));
        let fork = vec![meta("child", Some("parent")), usage(7, 2, AT)];
        assert_eq!(
            output("bare-fork", &[("sessions/fork.jsonl", fork)]),
            Some(4)
        );
    }

    #[test]
    fn codex_models_limits_and_unreadable_logs() {
        let turn = |model: &str| json!({"type": "turn_context", "payload": {"model": model}});
        let mut limited = usage(3, 3, AT);
        limited["payload"]["rate_limits"] =
            json!({"limit_id": "spark", "primary": {"used_percent": 12, "window_minutes": 15}});
        let mut week = usage(2, 2, AT);
        week["payload"]["rate_limits"] =
            json!({"limit_id": "codex", "primary": {"used_percent": 24, "window_minutes": 10080}});
        let rows = vec![
            meta("s", None),
            turn("gpt-6-astra"),
            week,
            turn("gpt-6.1-sol"),
            limited,
        ];
        let (records, limits, _) = codex_records("models", &[("sessions/a.jsonl", rows)]);
        assert_eq!(
            json!([records[0]["model"], records[1]["model"]]),
            json!(["gpt-6-astra", "gpt-6.1-sol"])
        );
        assert_eq!(
            json!([records[0]["session"], records[1]["session"]]),
            json!(["s", "s"])
        );
        let limits = codex_log_limits(limits).unwrap_or_default();
        assert_eq!(
            json!([limits["windows"][0]["label"], limits["windows"][1]["label"]]),
            json!(["7-day", "spark: 15-minute"])
        );
        let dirs = home("truncated");
        let cut = lines(&[meta("s", None), usage(2, 2, AT)]) + "{\"type\":";
        write_bytes(&dirs.codex.join("sessions/a.jsonl"), cut.as_bytes());
        write_bytes(&dirs.codex.join("sessions/b.jsonl.zst"), b"corrupt");
        let mut warnings = Vec::new();
        assert_eq!(
            dirs.scan_codex(None, &mut warnings)
                .map(|found| found.0.len()),
            Ok(1)
        );
        assert_eq!(
            warnings,
            ["Could not fully read b.jsonl.zst (ZstdError); totals may be incomplete."]
        );
    }

    fn assistant(id: &str, blocks: Value) -> Value {
        json!({"type": "assistant", "timestamp": "2026-11-01T10:00:00Z", "requestId": format!("r-{id}"),
               "cwd": "/p/demo", "message": {"id": id, "model": "claude-opus-5-5", "content": blocks,
                                             "usage": {"input_tokens": 10, "output_tokens": 5}}})
    }

    /// `{"name|tool": count}` of the events of one kind.
    fn counts(events: &[Value], kind: &str) -> Value {
        let mut out = json!({});
        for e in events.iter().filter(|e| e["kind"] == kind) {
            let slot = &mut out[format!("{}|{}", str_of(&e["name"]), str_of(&e["tool"]))];
            *slot = json!(slot.as_i64().unwrap_or(0) + 1);
        }
        out
    }

    #[test]
    fn activity_events_are_counted_once() {
        let (dirs, mut events, mut warnings) = (home("activity"), Vec::new(), Vec::new());
        let call =
            json!({"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}});
        let tool = |id: &str, name: &str, input: Value| json!({"type": "tool_use", "id": id, "name": name, "input": input});
        let more = json!([
            tool("t2", "Agent", json!({"subagent_type": "Explore"})),
            tool("t3", "Task", json!({})),
            tool("t4", "Skill", json!({"skill": "pdf"})),
            tool("t5", "mcp__rea__open_binary", json!({}))
        ]);
        let turn = json!({"type": "system", "subtype": "turn_duration", "durationMs": 90000, "uuid": "u1",
                          "timestamp": "2026-11-01T10:00:00Z"});
        let rows = [
            assistant("m1", json!([call])),
            assistant("m1", json!([call])),
            assistant("m2", more),
            turn.clone(),
            turn,
        ];
        write(&dirs.claude.join("projects/proj/s1.jsonl"), &rows);
        let records = dirs.scan_claude(Some(&mut events), &mut warnings);
        assert_eq!(records.as_ref().map(Vec::len), Ok(2));
        assert_eq!(dirs.scan_claude(None, &mut warnings), records);
        let tools = json!({"Bash|Claude Code": 1, "Agent|Claude Code": 1, "Task|Claude Code": 1, "Skill|Claude Code": 1,
                           "MCP: rea|Claude Code": 1});
        assert_eq!(counts(&events, "tool"), tools);
        assert_eq!(
            counts(&events, "agent"),
            json!({"Explore|Claude Code": 1, "general-purpose|Claude Code": 1})
        );
        assert_eq!(counts(&events, "skill"), json!({"pdf|Claude Code": 1}));
        assert_eq!(counts(&events, "mcp"), json!({"rea|Claude Code": 1}));
        let out = aggregate(
            &json!(records.unwrap_or_default()),
            &json!({}),
            Some(&json!(events)),
        )
        .unwrap_or_default();
        assert_eq!(out["longest_turn_ms"], json!({"Claude Code": 90000}));
        assert_eq!(out["activity"].get("turn"), None);

        let ts = "2026-11-01T10:05:00Z";
        let call = |id: &str, name: &str, namespace: Value, when: &str| {
            json!({"type": "response_item", "timestamp": when,
                   "payload": {"type": "function_call", "name": name, "namespace": namespace, "call_id": id}})
        };
        let main = [
            json!({"type": "session_meta", "timestamp": ts, "payload": {"id": "s", "cwd": "/p/x"}}),
            call("c1", "exec", json!(null), ts),
            call("c1", "exec", json!(null), ts),
            call("c2", "js", json!("mcp__cua_repl"), ts),
            call("c3", "spawn_agent", json!("collaboration"), ts),
        ];
        write(&dirs.codex.join("sessions/a.jsonl"), &main);
        // a fork: copied parent history before its start is not counted again
        let fork = json!({"id": "f", "forked_from_id": "s", "timestamp": ts});
        let fork = [
            json!({"type": "session_meta", "timestamp": ts, "payload": fork}),
            call("c9", "exec", json!(null), "2026-11-01T10:00:00Z"),
            call("c10", "exec", json!(null), "2026-11-01T10:06:00Z"),
        ];
        write(&dirs.codex.join("sessions/b.jsonl"), &fork);
        let mut events = Vec::new();
        assert!(dirs.scan_codex(Some(&mut events), &mut warnings).is_ok());
        assert_eq!(
            counts(&events, "tool"),
            json!({"exec|Codex": 2, "MCP: cua_repl|Codex": 1, "spawn_agent|Codex": 1})
        );
        assert_eq!(counts(&events, "mcp"), json!({"cua_repl|Codex": 1}));
        assert_eq!(counts(&events, "agent"), json!({"Codex subagent|Codex": 1}));

        let ms = Local
            .with_ymd_and_hms(2026, 11, 1, 9, 0, 0)
            .single()
            .map_or(0, |t| t.timestamp_millis());
        let typed = [
            json!({"display": "/model opus", "timestamp": ms}),
            json!({"display": "fix the bug", "timestamp": ms}),
            json!({"display": "/c/Users/me/file.txt is broken", "timestamp": ms}),
        ];
        write(&dirs.claude.join("history.jsonl"), &typed);
        write(
            &dirs.codex.join("history.jsonl"),
            &[json!({"text": "/review please", "ts": ms / 1000})],
        );
        let mut events = Vec::new();
        assert_eq!(dirs.scan_commands(&mut events, &mut warnings), Ok(()));
        let day = json!("2026-11-01");
        let commands = [
            event("Claude Code", &day, "command", json!("/model")),
            event("Codex", &day, "command", json!("/review")),
        ];
        assert_eq!(events, commands);
    }

    #[test]
    fn live_checks_read_the_saved_logins() {
        let dirs = home("live");
        let no_call: Fetch = &|_, _, _| (Value::Null, json!("called"));
        assert_eq!(
            dirs.codex_live_with(no_call),
            Ok((Value::Null, json!("no Codex login found")))
        );
        assert_eq!(
            dirs.claude_live_with(T0, no_call),
            Ok((Value::Null, json!("no Claude Code login found")))
        );
        write(
            &dirs.codex.join("auth.json"),
            &[json!({"tokens": {"access_token": "test"}})],
        );
        let window =
            json!({"used_percent": 30, "limit_window_seconds": 900, "reset_at": 1793540000});
        let body = json!({"plan_type": "business", "rate_limit": {"primary_window": window},
                          "code_review_rate_limit": {"primary_window": window}, "credits": {"balance": "42.5"},
                          "spend_control_reached": true});
        let codex: Fetch = &|url, headers, app| {
            let sent = [
                ("Authorization", "Bearer test".to_string()),
                ("ChatGPT-Account-Id", String::new()),
                ("User-Agent", "codex_cli_rs".into()),
                ("Accept", "application/json".into()),
            ];
            assert_eq!((url, headers, app), (CODEX_USAGE_URL, &sent[..], "Codex"));
            (body.clone(), Value::Null)
        };
        let windows = json!([{"label": "15-minute", "used_percent": 30, "resets_at": 1793540000},
                             {"label": "Code review: 15-minute", "used_percent": 30, "resets_at": 1793540000}]);
        let details = json!(["Credit balance: 42.5", "Workspace spend control reached"]);
        let live = json!({"plan": "Business", "windows": windows, "details": details});
        assert_eq!(dirs.codex_live_with(codex), Ok((live, Value::Null)));
        let empty = "Codex returned no usage windows; showing the last known reading";
        assert_eq!(
            dirs.codex_live_with(&|_, _, _| (json!({}), Value::Null)),
            Ok((Value::Null, json!(empty)))
        );
        let offline = json!("live check failed (offline?)");
        assert_eq!(
            dirs.codex_live_with(&|_, _, _| (Value::Null, offline.clone())),
            Ok((Value::Null, offline.clone()))
        );

        let login = json!({"accessToken": "x", "expiresAt": 9e15, "rateLimitTier": "default_claude_max_20x"});
        write(
            &dirs.claude.join(".credentials.json"),
            &[json!({"claudeAiOauth": login})],
        );
        let mut body = json!({"limits": [{"kind": "weekly_all", "percent": 30, "resets_at": "2026-11-03T00:00:00Z"},
                                         {"kind": "weekly_model", "scope": {"model": {"display_name": "Opus"}}}],
                              "seven_day_breakdown": {"rows": [{"key": "claude_code", "display_name": "Claude Code", "percent": 25}]},
                              "spend": {"enabled": true, "used": {"amount_minor": 1234, "currency": "USD", "exponent": 2}}});
        let claude = |body: Value| {
            dirs.claude_live_with(T0, &move |url, headers, app| {
                let sent = [
                    ("Authorization", "Bearer x".to_string()),
                    ("anthropic-beta", "oauth-2025-04-20".into()),
                    ("User-Agent", "claude-cli".into()),
                    ("Accept", "application/json".into()),
                ];
                assert_eq!(
                    (url, headers, app),
                    (CLAUDE_USAGE_URL, &sent[..], "Claude Code")
                );
                (body.clone(), Value::Null)
            })
        };
        let windows = json!([{"label": "7-day, all models", "used_percent": 30, "resets_at": 1793664000.0},
                             {"label": "7-day, Opus", "used_percent": 0, "resets_at": null}]);
        let breakdown = json!([{"key": "claude_code", "name": "Claude Code", "percent": 25}]);
        let mut live = json!({"plan": "Max 20x", "windows": windows, "breakdown": breakdown,
                              "spend": {"used": 12.34, "currency": "USD"}});
        assert_eq!(claude(body.clone()), Ok((live.clone(), Value::Null)));
        body["spend"]["enabled"] = json!(false);
        live["spend"] = Value::Null;
        assert_eq!(claude(body), Ok((live, Value::Null)));
        let old = json!({"five_hour": {"utilization": 12.5, "resets_at": "2026-11-03T00:00:00Z"}, "seven_day": {}});
        let windows =
            json!([{"label": "5-hour session", "used_percent": 12.5, "resets_at": 1793664000.0}]);
        assert_eq!(
            claude(old).map(|(live, _)| live["windows"].clone()),
            Ok(windows)
        );
        let expired = json!("Claude Code login expired, open Claude Code to renew it");
        assert_eq!(
            dirs.claude_live_with(9e12 + 1.0, no_call),
            Ok((Value::Null, expired))
        );
    }

    #[test]
    fn plan_limits_and_dashboard_match_the_original() {
        let want = expected();
        for name in ["plan_fresh", "plan_cached", "plan_broken"] {
            let (dirs, case, mut log) = (home(name), &want[name], Vec::new());
            match &case["cache"] {
                Value::Null => {}
                Value::String(text) => {
                    write_bytes(&dirs.store.join("limits_cache.json"), text.as_bytes())
                }
                cache => write(
                    &dirs.store.join("limits_cache.json"),
                    std::slice::from_ref(cache),
                ),
            }
            let mut check = |service: &str| {
                let found = &case[if service == "Codex" {
                    "codex"
                } else {
                    "claude"
                }];
                Ok((found[0].clone(), found[1].clone()))
            };
            let mut say = |line: &str| log.push(line.to_string());
            let entries = dirs.plan_limits_with(
                &want["codex"][1],
                &mut say,
                &mut check,
                "2026-11-03T00:00:00.000Z",
            );
            assert_eq!(
                entries.map(|e| e.to_string()),
                Ok(case["entries"].to_string()),
                "{name}"
            );
            assert_eq!(json!(log), case["log"]);
            let saved = read_json(&dirs.store.join("limits_cache.json"));
            assert_eq!(
                saved.map(|s| s.to_string()),
                Some(case["saved"].to_string())
            );
        }
        let page = render_dashboard(TEMPLATE, &want["collect"]).replace("\r\n", "\n");
        assert!(want["dashboard"] == page && page.contains("const DATA = {\"day\": ["));
    }

    /// The recorded value with the bridge's marker for whole floats read back.
    fn decoded(v: &Value) -> Value {
        match v {
            Value::Array(items) => items.iter().map(decoded).collect(),
            Value::Object(map) => match map.get("$float").and_then(Value::as_str) {
                Some(float) => json!(float.parse::<f64>().unwrap_or_default()),
                None => object(map.iter().map(|(k, v)| (k.clone(), v)), decoded),
            },
            other => other.clone(),
        }
    }

    /// `aggregate` is not bridged, as the bridge cannot tell `events=None`, which fails, from the
    /// default `()`. Its cases, as rust-anything recorded them from the original while it was
    /// bridged, are replayed here and compared as `rust-anything diff` compares.
    #[test]
    fn aggregate_matches_its_recorded_cases() {
        let cases = include_str!("../fixture/aggregate-cases.jsonl").lines();
        let mut count = 0;
        for case in cases.filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
            let args = decoded(&case["args"]);
            let events = args.get(2).filter(|e| **e != json!({"$undefined": true}));
            let got = aggregate(&args[0], &args[1], events);
            let got = match got {
                Ok(out) => json!({"ok": out}).to_string(),
                Err(error) => error.split(':').next().unwrap_or_default().to_string(),
            };
            let error = case["error"].as_str().and_then(|e| e.split(':').next());
            let want = error.map_or_else(
                || json!({"ok": decoded(&case["ok"])}).to_string(),
                str::to_string,
            );
            assert!(
                got == want || error.is_some() && got == "TypeError",
                "{}: {got}",
                case["args"]
            );
            count += 1;
        }
        assert_eq!(count, 219);
    }

    /// Everything the scans give for the fixture home (`fixture/make_home.py`), against what the
    /// original gave (`fixture/make_expected.py`). Runs when `TRACKER_FIXTURE_HOME` names it.
    #[test]
    fn fixture_home_matches_the_original() {
        let Some(root) = std::env::var_os("TRACKER_FIXTURE_HOME").map(PathBuf::from) else {
            return;
        };
        let (want, mut dirs) = (expected(), home("fixture"));
        (dirs.claude, dirs.codex) = (root.join(".claude"), root.join(".codex"));
        dirs.here = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (mut events, mut warnings, mut log) = (Vec::new(), Vec::new(), Vec::new());
        let claude = dirs.scan_claude(Some(&mut events), &mut warnings);
        assert_eq!(
            claude.map(|records| json!(records).to_string()),
            Ok(want["claude"].to_string())
        );
        let codex = dirs.scan_codex(Some(&mut events), &mut warnings);
        assert_eq!(
            codex.map(|found| json!(found).to_string()),
            Ok(want["codex"].to_string())
        );
        assert_eq!(dirs.scan_commands(&mut events, &mut warnings), Ok(()));
        assert_eq!(json!(events).to_string(), want["events"].to_string());
        assert_eq!(json!(warnings), want["warnings"]);
        let titles = dirs.codex_titles().unwrap_or_default();
        for (path, info) in want["sessions"].as_object().into_iter().flatten() {
            let found = codex_session_info(&dirs.codex.join("sessions").join(path), &titles);
            assert_eq!(
                found.map(|info| info.to_string()),
                Ok(info.to_string()),
                "{path}"
            );
        }
        let working = dirs.working_details(100_000, Some(T0 + 30.0));
        assert_eq!(
            working.map(|sessions| json!(sessions).to_string()),
            Ok(want["working"].to_string())
        );
        let mut api = |log: Log| {
            log("Reading api ...");
            json!({"stub": "api"})
        };
        let mut openai = |log: Log| {
            log("Reading openai ...");
            json!({"stub": "openai"})
        };
        let mut data = dirs
            .collect(
                &mut |line| log.push(line.to_string()),
                &mut api,
                &mut openai,
            )
            .unwrap_or_default();
        for clock in ["generated", "today", "now_epoch"] {
            assert!(data[clock].is_string() || data[clock].is_i64());
            data[clock] = want["collect"][clock].clone();
        }
        assert_eq!(data.to_string(), want["collect"].to_string());
        assert_eq!(json!(log), want["collect_log"]);
    }
}
